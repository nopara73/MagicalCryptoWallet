# Compact filter migration handoff

Status: component included in the published application host; a bounded matching
caller/handler review patch is prepared separately. Verification date:
2026-10-02, Asia/Singapore. Worker chat: `01a0fc2a-a793-7522-ac10-b5832440dec6`.
Coordinator owns integration dispatch; this document does not claim that managed
callers, networking, storage, or the production wallet have migrated.

## Commits and file ownership

- Initial implementation: `e06082994fbf44e2b66cdffad66d3bb98f4e0215`.
- Final implementation and lint verifier: `440cdc8cffb20c754c8036326805d294f92f620a`.
- Required first-party SHA256 implementation from the encoding worker:
  `6365f3244d23b801c0bb36581967caad8aae968e`. That worker owns its source.
- The handoff-document commit is recorded as `commit` in the atomic coordination
  record `.artifacts/mcw-coordination/handoffs/compact-filters.json`; the two
  implementation commits above are recorded separately.

Owned files:

- `mcw/src/compact_filters.rs`
- `mcw/tests/compact_filters_conformance.rs`
- `mcw/tests/compact_filters_vectors.inc`
- `mcw/tests/compact_filters_reference.py`
- `mcw/tests/compact_filters_verify.ps1`
- `mcw/tests/compact_filters_bridge_handler.inc`
- `mcw/tests/compact_filters_managed_adapter.inc`
- `mcw/tests/compact_filters_host_prepare.py`
- `mcw/tests/compact_filters_host_wiring.patch`
- `mcw/tests/compact_filters_host_probe.inc`
- `mcw/tests/compact_filters_host_verify.ps1`
- `Contrib/McwMigration/Handoffs/compact-filters.md`

No tracked Cargo manifests, module declarations, host commands, bridge code,
managed adapters, network/filter-store code, packaging, or launch paths were edited.
The proposed four-file integration patch is an owned test/handoff artifact;
it was applied only in ignored synthetic verification snapshots.
There is no additional Cargo package or shipping executable. Temporary test
crates, executables, reference downloads, and evidence are under the worker's
ignored `.artifacts` directory, not in shipping sources.

## Implemented behavior

The module implements SipHash-2-4, 128-bit multiply-high range mapping, GCS
construction, sorted delta coding, MSB-first Golomb-Rice bit streams, canonical
CompactSize counts, decoding, single-query matching, set-intersection matching,
per-query results, and BIP157 filter hashes/header commitments. All byte/bit
processing is first-party Rust using `std`, with `#![forbid(unsafe_code)]`.

`Params::BASIC` uses filter type `0x00`, P=19 and M=784931. `Params::new(p, m)`
supports P=0..=63 and M=1..=u32::MAX; counts must be less than 2^32. Custom
parameters include the legacy P=20/M=1048576 format, but callers must supply
them explicitly. P/M are not serialized and must not be inferred from bytes.

Basic construction accepts all transaction output scripts and the spent output
scripts for non-coinbase inputs. It omits empty scripts, omits output scripts
whose first byte is OP_RETURN (0x6a), and deduplicates the remaining union by
exact bytes before computing N. Spent scripts only have the empty-script rule.
Unparseable/nonstandard scripts are retained when their bytes qualify. The
caller remains responsible for complete prevout data and excluding coinbase
inputs. The generic GCS builder accepts empty byte strings as set elements.

Distinct scripts with identical mapped values produce repeated values/zero
deltas. Query collisions produce legitimate false positives. A match requires
the caller to inspect the block; it is not proof of a relevant transaction.

All hashes use raw internal/wire 32-byte order. `basic_key` selects the first
16 bytes of that block hash, with little-endian SipHash key words. Display hex
must be reversed by the existing encoding/adaptation layer before use. Filter
hashes include the CompactSize prefix. Headers hash `filter_hash || previous`
with first-party double-SHA256. Genesis uses a zero previous header. Header
chaining is ordered calculation only; it does not authenticate blocks, peers,
checkpoints, anchors, heights, or reorgs.

## Public API

Every fallible API returns `Result<_, compact_filters::Error>`. `Hash` wraps
`bitcoin_encoding::Error` without replacing its diagnostics. Only the internal
`bitcoin_encoding::double_sha256(&[u8]) -> Result<[u8;32], Error>` service is
needed from the encoding worker.

```rust
pub struct Limits {
    pub max_filter_bytes: usize,
    pub max_elements: u32,
    pub max_queries: usize,
    pub max_input_bytes: usize,
}

pub fn encode_basic(
    block_hash: &[u8;32], output_scripts: &[&[u8]],
    spent_scripts: &[&[u8]], limits: Limits,
) -> Result<Vec<u8>, Error>;

pub fn encode_gcs(
    key: &[u8;16], params: Params,
    elements: &[&[u8]], limits: Limits,
) -> Result<Vec<u8>, Error>;

pub fn encode_mapped_values(
    params: Params, values: &[u64], limits: Limits,
) -> Result<Vec<u8>, Error>;

impl<'a> GcsFilter<'a> {
    pub fn parse_basic(
        encoded: &'a [u8], block_hash: &[u8;32], limits: Limits,
    ) -> Result<Self, Error>;
    pub fn parse(
        encoded: &'a [u8], key: [u8;16], params: Params, limits: Limits,
    ) -> Result<Self, Error>;
    pub fn count(&self) -> u32;
    pub fn params(&self) -> Params;
    pub fn key(&self) -> &[u8;16];
    pub fn encoded(&self) -> &'a [u8];
    pub fn mapped_values(&self) -> Result<Vec<u64>, Error>;
    pub fn matches(&self, script: &[u8]) -> Result<bool, Error>;
    pub fn match_any(&self, scripts: &[&[u8]]) -> Result<bool, Error>;
    pub fn match_queries(&self, scripts: &[&[u8]]) -> Result<Vec<bool>, Error>;
    pub fn filter_hash(&self) -> Result<[u8;32], Error>;
    pub fn filter_header(&self, previous: &[u8;32]) -> Result<[u8;32], Error>;
}

pub fn basic_key(block_hash: &[u8;32]) -> [u8;16];
pub fn siphash24(key: &[u8;16], message: &[u8]) -> u64;
pub fn map_into_range(hash: u64, range: u64) -> u64;
pub fn filter_header_from_hash(
    filter_hash: &[u8;32], previous: &[u8;32],
) -> Result<[u8;32], Error>;
pub fn chain_filter_headers(
    filter_hashes: &[[u8;32]], previous: &[u8;32], max_headers: usize,
) -> Result<Vec<[u8;32]>, Error>;
```

`GcsFilter` borrows only the encoded filter. Query/script bytes are never stored.
The immutable borrow prevents mutation after validation in safe Rust. Queries
stream decoded values and allocate only bounded query hashes/results, not all
filter values. `match_queries` preserves query order and duplicates.

## Canonical and bounded input contract

Defaults are 4,000,000 serialized bytes, 1,000,000 input/filter elements,
1,000,000 queries, and 32,000,000 aggregate script/query bytes. These are local
resource policy, not consensus or P2P message-size limits. Supply an explicit
policy at the application boundary. Construction counts/bytes include duplicate,
empty, and excluded inputs before deduplication/exclusion. Query byte limits
apply per call. Even empty-filter queries receive size/count validation.

Parsing validates the complete stream before a `GcsFilter` is returned: minimal
count encoding, bounded N and bytes, sufficient minimum bits, exactly N values,
all values below N*M, no extra full bytes, and zero final padding. Empty filters
are exactly `00`. Unary quotients are range-bounded before shifting, and
cumulative sums cannot overflow. Strict zero-padding rejection is intentional;
Bitcoin Core's cited constructor checks consumed bytes but does not check every
padding bit. Encodings generated according to BIP158 have zero padding.

Malformed suffixes cannot become valid because an earlier value matched or the
query list was empty. Construction precomputes total encoded size using u128
before allocating/writing unary runs. Bounded vectors use `try_reserve_exact`;
allocation errors return `AllocationFailed`. Production code contains no I/O,
native handles, IPC, platform bindings, recursion, C# types, wallet secrets, or
external imports.

## Reserved bridge operation proposal

Range: `0x0700-0x07FF`. These operations are proposals for the host owner;
no bridge handlers or protocol bytes were added by this worker.

| Proposed operation | Domain call and inputs | Result |
| --- | --- | --- |
| `0x0700` validate/decode basic | raw block hash + full encoded filter + policy | N / mapped values, or error |
| `0x0701` match basic | same + one script | boolean |
| `0x0702` match any basic | same + script list | boolean |
| `0x0703` match queries basic | same + script list | ordered booleans |
| `0x0704` build basic | raw block hash + output scripts + non-coinbase spent scripts | encoded filter |
| `0x0705` hash basic filter | validated basic filter | raw 32-byte filter hash |
| `0x0706` next filter header | raw filter hash + raw previous header | raw 32-byte header |
| `0x0707` chain headers | ordered raw filter hashes + previous anchor + bound | raw header list |
| `0x0708` build custom GCS | 16-byte key + explicit P/M + elements + policy | encoded GCS |
| `0x0709` match custom GCS | 16-byte key + explicit P/M + encoded GCS + scripts | ordered booleans |

Use host-owned framing, payload/list bounds, error mapping, and adapters. A
network-supplied block hash must first match the authenticated block-header
chain; do not let peers choose the matching key. Preserve the existing managed
validation order and canonical-block-hash regression while migrating callers.

## Verification and reference evidence

Rust 1.99.0 / edition 2024, Windows x64:

- All 19 compact-filter tests passed in debug and optimized builds, both with
  overflow checks and compiler warnings denied.
- Clippy library and test metadata checks passed with `-D warnings` and
  `clippy::all`; formatting checks passed.
- All 10 official BIP158 testnet vectors reproduced exact filter bytes and
  BIP157 headers using the **actual** encoding-worker module, not a stub/callback.
- Filter hashes also match independent Python `hashlib` results.
- All 64 original-author SipHash-2-4 vectors passed, plus independent lengths
  64, 65, 127, 128, 255, 256, 257, 511, 512, 1024 and 4096.
- 90 independently generated GCS cases cover P=0/1/2/5/19/20/31/63,
  M=1/17/31/42/79/784931/1048576/u32::MAX, empty sets, long scripts,
  duplicate inputs, hash collisions, and single/multi-query results.
- Exhaustively tested 327,680 short hostile encodings (counts 0..4 and every
  two-byte body). Accepted encodings reencode identically and match independently
  inspected values. Bit mutations of every official filter also preserve that
  property when accepted. Truncation, count overflow/nonminimal counts, unary
  bounds, value-range/cumulative bounds, extra bytes, padding, malformed suffixes,
  resource limits, and CompactSize 252/253/65535/65536 transitions are covered.

The independent generator uses Python standard library arithmetic, bit strings,
an independently structured SipHash implementation, and `hashlib`. It independently
extracts output scripts from full official blocks including witness data, verifies
block hashes and prevout counts, and confirms each expected filter/header before
generating fixture constants. Python/MSVC/SDK tools are development verification
tools, not runtime dependencies of mcw. Tests themselves need no Python or network
when using the committed Rust fixture constants.

Reference inputs were background-downloaded into the worker's ignored directory
and verified by content/hash:

| Primary source | Retrieved file SHA256 |
| --- | --- |
| [BIP158 testnet vectors](https://github.com/bitcoin/bips/blob/master/bip-0158/testnet-19.json) | `d9049756f744e561b882a8eff507582fb7cd74ed9cf5542bdac58257449ee2a2` |
| [Bitcoin Core blockfilter vectors](https://github.com/bitcoin/bitcoin/blob/master/src/test/data/blockfilters.json) | Same bytes/hash as the BIP158 vectors |
| [SipHash authors' 64-bit vectors](https://github.com/veorq/SipHash/blob/master/vectors.h) | `212c44114a63c6d84710b8627f3bc5ce155698accf2ea7bbff1c4b69c9f48d31` |

Specifications/review: [BIP158](https://github.com/bitcoin/bips/blob/master/bip-0158.mediawiki),
[BIP157](https://github.com/bitcoin/bips/blob/master/bip-0157.mediawiki), and
[Bitcoin Core construction/decoding](https://github.com/bitcoin/bitcoin/blob/master/src/blockfilter.cpp).
Generated fixture SHA256 (LF):
`69f9a893f7d1ee3a293e81fe6f5b83f77908851f32fc66deb3586de60a573ce6`.
Tested actual encoding-source SHA256:
`4e59e9210317f6c75f1196d22892202b3c72f62adcf9d05be88c4c3c66925ec0`.
Final compact-filter source SHA256 after canonical LF normalization:
`9cf5a5dc272298f5831b26fe76271025a1dc49e34a432d5c8b4b5a947912c255`.
Line-ending conversions can change physical source hashes without changing Git blobs.

### Reproduction

From the worker checkout, the exact successful command was:

```powershell
$taskTools = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-tools\rustup\toolchains\1.99.0-x86_64-pc-windows-msvc\bin'
& .\mcw\tests\compact_filters_verify.ps1 `
  -ToolchainBin $taskTools `
  -CoordinationRoot 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-coordination' `
  -EncodingSource 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-bitcoin-encoding\mcw\src\bitcoin_encoding.rs' `
  -ExpectedEncodingSha256 4e59e9210317f6c75f1196d22892202b3c72f62adcf9d05be88c4c3c66925ec0 `
  -Linker 'C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\MSVC\14.51.36231\bin\Hostx64\x64\link.exe' `
  -NativeLibraryPaths @(
    'C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\MSVC\14.51.36231\lib\onecore\x64',
    'C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\ucrt\x64',
    'C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64'
  ) `
  -ReferenceDirectory .artifacts\compact-filters-reference `
  -Python 'C:\Python314\python.exe'
```

Without `-EncodingSource`, the verifier uses the checkout's actual
`mcw/src/bitcoin_encoding.rs`. `-ReferenceDirectory` is optional: omitting it
runs conformance against committed constants. To regenerate the constants, supply
the two source files above as `testnet-19.json` and `siphash-vectors.h` in that
directory. The generator requires their exact documented hashes. The verifier
creates only an ignored temporary test crate, uses a shared build-slot lock,
defers below 2 GiB free memory, and confirms the SHA source did not change during
verification. No toolchain was installed or modified by this worker.

Evidence under the isolated checkout's `.artifacts/compact-filters-evidence/`:
`verification.json`, `debug-tests.txt`, `optimized-tests.txt`,
`clippy-library.txt`, `clippy-tests.txt`, `independent-reference.txt`,
`windows-dependencies.txt`, and target metadata. Windows test executables used
static CRT and the installed native MSVC/Windows SDK link libraries. The optimized
test executable's PE import audit lists only `api-ms-win-core-synch-l1-2-0.dll`,
`bcryptprimitives.dll`, `KERNEL32.dll`, `ntdll.dll`, and `USERENV.dll`, all Windows
system libraries. There is no companion CRT/package library. This is component
test-executable evidence, not an audit of the host owner's packaged wallet.
No additional binary is shipped.

Only Windows target std is currently installed in the shared toolchain.
Windows x64 metadata compilation and test execution passed. Linux x64/ARM64 and
macOS x64/ARM64 checks are explicitly **unavailable / unverified**, not passes.
They must be typechecked, linked and run in the host owner's five-target pipeline.

## Retained dependencies and remaining live callers

Audited latest remote master before implementation at
`82127991068522210cdcf77080dc9b819502e486`, then rechecked the caller graph at
published revision `440cdc8cffb20c754c8036326805d294f92f620a`. The managed client
remains transitional. NBitcoin `10.0.13` is retained; its lock entry depends on
`Microsoft.Extensions.Logging.Abstractions` (resolved 10.0.12) and
`Newtonsoft.Json` (resolved 13.0.4). Many live
transaction, keys, block, network, and wallet callers also use NBitcoin (105
managed source files matched explicit imports/references at both revisions).
This work cannot remove the complete package or those transitive dependencies.

| Remaining responsibility/caller | Required integration |
| --- | --- |
| `MagicalCryptoWallet/Backend/Models/FilterModel.cs` | Replace `GolombRiceFilter` decoding/serialization and key selection with domain data + validated Rust operations |
| `MagicalCryptoWallet/Wallets/WalletFilterProcessor.cs` | Replace `Filter.MatchAny` with Rust multi-query matching; preserve block retrieval, transaction processing, and wallet progress semantics |
| `MagicalCryptoWallet/BitcoinP2p/CompactFilterBehavior.cs` | Replace filter construction/`GetHeader`; preserve expected-header and authenticated block-hash checks, peer/range validation, reorg and cancellation handling |
| `MagicalCryptoWallet/BitcoinP2p/FilterSynchronizationState.cs` | Replace managed SHA256 header commitment calculation; retain chain/range/anchor policy |
| `MagicalCryptoWallet/Blockchain/BlockFilters/FilterCheckpoints.cs` | Preserve exact published filter/checkpoint bytes; route codec interpretation through explicit basic parameters |
| `MagicalCryptoWallet/Stores/BlockFilterSqliteStorage.cs` | Still calls `FilterModel.Create` and stores filter/header bytes; preserve persisted data and storage schema during adapter migration |
| `Contrib/Utils/GenerateCheckpoint.cs` | Retained checkpoint-generation helper constructs `GolombRiceFilter` and calls `GetHeader`; migrate its codec/hash operations before claiming every format caller is gone |
| `MagicalCryptoWallet/Stores/FilterStore.cs`, `Services/Synchronizer.cs`, wallet/filter iterator/header-chain classes | Retain storage/network/state responsibilities; no Rust replacements are claimed here |
| Managed filter-store, filter-provider, header-chain, iterator and compact-filter integration tests | Keep equivalent behavior tests while changing adapters; custom P=20/M=1048576 test fixtures need explicit GCS parameters |

Direct managed Golomb-Rice test users found across the full checkout are
`MagicalCryptoWallet.Tests/UnitTests/Wallet/FilterProcessor/BlockFilterIteratorTests.cs`,
`MagicalCryptoWallet.Tests/UnitTests/Stores/BlockFilterSqliteStorageTests.cs`,
`MagicalCryptoWallet.Tests/UnitTests/Services/FilterProvidersTests.cs`, and
`MagicalCryptoWallet.Tests/UnitTests/Services/CompactFilterBehaviorTests.cs`.

The initial read-only host-checkout observation found `mcw/Cargo.toml`: empty dependencies,
dev-dependencies and build-dependencies sections. Its `Cargo.lock` contained
only `mcw` 0.1.0. This is a read-only host-checkout snapshot, not a claim that its
uncommitted host/packaging work was part of those component commits. This worker adds zero
external Cargo/runtime libraries; `bitcoin_encoding` is an internal first-party
module ultimately compiled into the same mcw executable.

## Integration and removal acceptance checks

1. Published host pin `b28331b8dfae53acdd1b25c77780f1a47762df2a` includes
   the encoding and compact-filter module declarations. The actual Cargo
   conformance run passed all 19 tests. Keep this check in the host pipeline.
2. Add host-owned bridge operations/adapters with framing/count/byte limits;
   domain code remains unaware of IPC, handles and managed types. Verify error
   propagation and raw-hash order using all official vector cases end to end.
3. Route every codec/hash/matching caller listed above to the domain APIs;
   preserve canonical block identity, trusted anchors, complete stream validation,
   legacy test parameters, checkpoint bytes, storage compatibility and reorg
   behavior. Run existing managed filter tests and synthetic regtest sync cases.
4. Typecheck, link and run the code on Windows x64, Linux x64/ARM64, macOS
   x64/ARM64; check release import/runtime dependencies in the single mcw host.
   The present worker evidence does not establish those four missing target runs.
5. Search production callers for `GolombRiceFilter`, `GolombRiceFilterBuilder`,
   `Filter.MatchAny`, `GetHeader`, and managed filter-specific SHA calls. Only
   declare this responsibility removed after those production paths are migrated.
   Remove NBitcoin only when all of its other live responsibilities are replaced.
6. Preserve wallet/filter state and other agents' scope. Only the coordinator
   requests incorporation when the host-owner chat is idle; the ready machine
   record is evidence and does not authorize edits to host-owned files.

## Published-host verification and bounded caller patch

The verification baseline is remote-master commit
`b28331b8dfae53acdd1b25c77780f1a47762df2a`, including published host foundation
`989cf2a2df22d23837c1aa328e29abfd33c9b9c8`. The owned review artifact
`mcw/tests/compact_filters_host_wiring.patch` proposes exactly these four paths:

| Proposed owner change | Behavior |
| --- | --- |
| `mcw/src/bridge.rs` | Decode bounded `0x0702` requests, fully validate a basic filter, call first-party `match_any`, return a canonical boolean or service error |
| `mcw/src/app.rs` | Dispatch `0x0702` in the existing permanent host |
| `MagicalCryptoWallet/Mcw/CompactFilters/McwCompactFilterMatcher.cs` | Typed async adapter with local count/size/null/hash validation, cancellation forwarding and strict boolean response validation |
| `MagicalCryptoWallet/Wallets/WalletFilterProcessor.cs` | Replace its one `Filter.MatchAny` call with the typed adapter; reject custom P/M before interpreting bytes as a basic filter |

The production `FilterModel.Create` constructor uses the default basic parameters.
[NBitcoin v10.0.13 source](https://github.com/MetacoSA/NBitcoin/blob/v10.0.13/NBitcoin/BIP158/GolombRiceFilter.cs)
confirms P=19/M=784931; its stale XML comment mentioning 20 is inconsistent with
its constants. Existing explicitly custom P=20/M=1048576 test fixtures must keep
their parameters when other codec callers migrate. This matching patch refuses
custom parameters rather than silently reinterpreting them.

`0x0702` payload, without the existing 16-byte application-frame header:

1. Raw wire-order block hash: 32 bytes.
2. Filter: length u32 LE followed by that many encoded filter bytes.
3. Query count: u32 LE.
4. Each query: length u32 LE followed by script bytes.

The entire payload is at most 1,048,560 bytes; queries are at most 65,536 and
filter elements at most 1,000,000. Exact consumption is required. Length/count
checks precede query-vector allocation and script copying. Response payload is
exactly one byte, 0 or 1. Malformed transport payloads use service error 1;
domain validation/matching failures use service error 2. The managed adapter
propagates these errors and never falls back to managed matching.

These are explicit local transport limits. A valid filter/query collection that
exceeds the combined frame budget is rejected; no streaming/chunking path or
unbounded wallet-key count is claimed by this patch.

Successful Windows verification on 2026-10-02 used
`mcw/tests/compact_filters_host_verify.ps1` and the existing shared toolchain,
native libraries, build-slot policy, and Python reference directory documented
above. Set `CARGO_HOME` and `RUSTUP_HOME` to the shared mcw tool directories,
then use the same parameters as the earlier command except omit
`-EncodingSource`/`-ExpectedEncodingSha256` and select the host verifier script.
The independent preparation tool checks the official vector-file hash before
constructing transport cases.

- Actual published-host Cargo conformance: 19 passed.
- Ignored snapshot with the proposed handler/dispatch patch: 19 passed; the
  actual `mcw` application executable built successfully.
- Cargo dependency graph: one package, zero external dependencies.
- Synthetic .NET 10 probe: the actual `ManagedApplicationHost.cs` and
  `IMcwApplicationServices.cs`, plus the proposed typed adapter, compiled with
  warnings as errors and no external NuGet packages. Only the adjacent
  termination-service type was stubbed; no wallet was opened.
- Typed adapter contract: 14 checks passed, including invalid boolean responses,
  null inputs, hash length, count/size limits, pre-cancellation and token forwarding.
- Real `mcw`/managed-probe round trip: 60 typed cases from all 10 official blocks,
  including 20 expected canonical-codec errors; 7 malformed inner payloads;
  40 concurrent requests; unknown-operation and pre-cancellation recovery;
  accepted maximum query count and frame size; adjacent oversized-frame rejection.
- The Windows GUI-subsystem host was launched hidden and explicitly awaited.
  Its successful exit is required before evidence is accepted. The synthetic
  probe has a two-minute watchdog; the verifier has a three-minute process limit.
- The three reserved existing patch-target files retained their original hashes.
  Patch application happened only in the ignored snapshot via `git apply --no-index`.
- Handler and patched host formatting, Python syntax, and PowerShell syntax passed.

Evidence is in the worker's ignored `.artifacts/compact-filters-host-evidence/`:
`host-verification.json`, both Cargo test logs, the host build log, managed probe
restore/build logs, `adapter-unit-results.json`, `host-roundtrip-results.json`,
and host stdout/stderr logs. Review-patch SHA256 after canonical LF normalization:
`6bbcf1fa9ac3ca73704f25f0d1d2ed8e7860b695c599699fa9703f501401051d`.
The preparation script can regenerate the patch against a newer pinned host
snapshot; rerun verification after changing its owner source or templates.
The verifier defaults to immediate deferral when both shared build slots are
occupied. `-WaitForBuildSlotSeconds 120` optionally retries for up to two minutes,
without holding another coordination lock; it rechecks free memory once admitted.

The proposed wallet caller file was patch-applied and inspected, but the full
managed wallet project and its existing filter tests were not built/run here.
Its end-to-end evidence covers the real host and typed managed transport, not
live wallet synchronization. Other filter parsing/header/checkpoint/storage
callers and NBitcoin remain in production. This worker did not apply the patch
to production, remove a package, create an extra shipping executable, modify an
active host-owner checkout, or establish five-target release readiness.
