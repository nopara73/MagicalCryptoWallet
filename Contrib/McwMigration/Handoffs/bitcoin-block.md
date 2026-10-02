# Bitcoin block and Merkle checkpoint

State (2026-10-02, Asia/Singapore): **block/Merkle component ready for bounded
caller integration; full synchronization migration stopped by the human scope
correction**. This checkpoint
does not replace the retained NBitcoin synchronization backend, remove its
package, authenticate headers, or constitute a production wallet release.

Worker: `bitcoin-block`, thread `01a0fc4c-0629-7a73-9462-6b0d23078e9c`.
Coordinator: `01a0fc1e-7c20-76d3-bf81-cb1f68c9adb7`.
Shared host owner: `01a0fbf5-89e2-7e90-9b98-50e3ff9bb5bc`.

## Published implementation and owned files

Implementation commit: `7f5f7ca8a297060d3826769aa9820948e89c7713`, normal direct
push to origin master, followed by fetched remote ancestry verification.
Canonical LF SHA-256 of `mcw/src/bitcoin_block.rs`:
`d4a2c4c9f70fb043ae8630ceecc5551ed11c6d4a96d52f36d94e35159129f78a`.
Actual first-party dependencies are bitcoin_encoding (implementation
`6365f3244d23b801c0bb36581967caad8aae968e`) and bitcoin_wire
(`d554088833b13446741310d8875abeb3e8a9eddc`); no duplicated SHA or transaction codec.

Owned checkpoint files are `mcw/src/bitcoin_block.rs`, `mcw/tests/bitcoin_block_*`
(conformance, reference/inventory/verification scripts and fixtures), and this
handoff. All primary source/reference/fixture hashes are recorded in
`mcw/tests/bitcoin_block_fixtures/manifest.json`. Source URLs, derivation and MIT
license retention are described in `SOURCES.md` and `BITCOIN-CORE-LICENSE.txt`.
Test/reference binaries and downloaded sources stay in ignored evidence folders.
No second Cargo package, shipping executable, external crate or native helper
runtime is added. The portable domain module has `forbid(unsafe_code)` and uses
only standard-library types and the two actual first-party modules.

## Domain API

All fallible operations return typed `bitcoin_block::Error` implementing Display
and std::error::Error. Public fields are revalidated by serialization/hash/proof
operations. Inputs are not trimmed, repaired or silently reinterpreted.

| Type/API | Contract |
| --- | --- |
| `BlockHeader { version: i32, previous_block_hash, merkle_root: [u8;32], time, bits, nonce: u32 }` | Exactly 80 wire bytes, signed version and little-endian integers; hash fields retain raw digest order. |
| `BlockHeader::decode / decode_prefix / encode / hash` | Exact decode rejects trailing data; prefix returns `(header, 80)`; encode returns `[u8;80]`; hash returns `BlockHash`. |
| `BlockHash([u8;32])`, Display, `from_display_hex` | Lowercase display reverses raw bytes; parsing accepts either hex letter case and requires exactly 64 ASCII hex bytes. |
| `Block { header, transactions: Vec<bitcoin_wire::Transaction> }` | Counted transaction container, using actual transaction prefix codecs and witness rules. |
| `Block::decode / decode_legacy / decode_prefix` | Exact/prefix bounds and aggregate decoded-memory budget; witness mode is default; explicit legacy mode handles zero-input legacy data. |
| `Block::sizes / serialize / serialize_legacy` | Stripped/total bytes and weight as data; canonical witness or stripped bytes; all opaque scripts/witnesses/signed amounts retain transaction codec semantics. |
| `Block::txids / merkle_root / check_merkle_root / merkle_proof` | txids exclude witness; opt-in root check rejects empty/mutated/mismatching data; proof build returns proof plus full-tree mutation evidence. |
| `merkle_root(&[TxId], &Limits)` | `MerkleRoot { hash: [u8;32], mutated: bool }`; duplicate-last at odd widths; equal real sibling pairs detected at every level; empty tree returns zero/unmutated. |
| `MerkleProof { transaction_count, transaction_index: u32, siblings: Vec<[u8;32]> }` | Siblings from leaf upward, including explicit duplicate-last nodes; build returns `(proof, MerkleRoot)`. |
| `MerkleProof::root / verify` | Checks count/index/exact branch depth, odd-width duplicate equality, visible real-sibling mutation and expected root. |
| `PartialMerkleTree { transaction_count: u32, hashes: Vec<[u8;32]>, flags: Vec<u8> }` | BIP37 depth-first hashes; flags least-significant bit first; count, CompactSize, bounds, complete consumption and traversal checked. |
| `PartialMerkleTree::build / decode / decode_prefix / encoded_len / serialize` | Canonical builder with full-input mutation rejection; exact original decoded flag bytes are retained through round trips. |
| `PartialMerkleTree::extract / extract_canonical` | `ExtractedMatches { merkle_root, matches: Vec<MerkleMatch { index, txid }>, bits_used }`; matches ordered by original position. |
| `MerkleBlock { header, tree }`, build/decode/prefix/extract/serialize | Header + BIP37 tree payload; extracted root must equal header root; no P2P envelope or Bloom filter implementation. |
| `CompactTarget::from_bits / checked_positive`, `encode_compact_target` | Checked numeric representation with big-endian 256-bit magnitude, sign, overflow and canonical flags; Core SetCompact/GetCompact truncation/normalization. |
| `BlockHeader::target(require_canonical)` | Rejects overflow, negative, zero and optionally noncanonical representations. No network powLimit/hash/work/chain acceptance. |

## Exact bounds and limits of the evidence

Defaults: block 4,000,000 bytes; 100,000 transactions/Merkle leaves; decoded memory
64,000,000 bytes; Merkle work 16,000,000 bytes; partial/merkleblock payload 1,000,000
bytes. Per-transaction limits remain explicit `bitcoin_wire::Limits` and apply
in addition to aggregate container limits. Prefix byte bounds apply only to the
consumed container, permitting following stream data. Count lower-byte checks
and fallible reservations precede allocation.

Decoded memory counts structure/vector elements and copied payload, excluding
allocator bookkeeping, caller input and separately bounded serialization buffers.
Block serialization may additionally hold one bounded transaction serialization.
Merkle root/proof construction accounts for copied leaves and branch storage;
Block wrappers subtract their txid array from the remaining work budget. Partial
builders account for all temporary levels and output arrays; extraction reserves
at most one match per supplied hash. Merkleblock limits include its 80-byte header.

Partial extraction follows Core's structural cap of 16,666 transactions, rejects
zero/hash-count/flag-count/traversal/identical-branch errors, and consumes every
hash and flag byte. Core permits arbitrary unused bits in the final consumed
byte; `extract` preserves that compatibility, while `extract_canonical` rejects
nonzero residual bits. Builders always use zero residual padding.

A partial proof or inclusion branch cannot reveal mutations hidden inside opaque
sibling subtrees. Its transaction-count metadata is not independently committed
by a Bitcoin root. Full-input builders/root checks provide full-tree mutation
evidence; callers must retain/check that evidence. A matching txid root does not
validate witness commitments, coinbase rules, scripts/signatures, monetary rules,
block weight, proof of work, chain selection, fees or synchronization.

Compact overflow magnitude is reported modulo 2^256, matching Core's data
interpretation; callers must use `checked_positive` before using a target. Small
exponents truncate the mantissa before sign/overflow reporting. Arbitrary targets
encoded back to nBits may lose low bytes; only exact canonical round trips set
`canonical=true`. Header encoding deliberately preserves any supplied bits.

## Independent verification

Rust 1.99.0, edition 2024. Actual-source library metadata and Clippy pass with
warnings denied; rustfmt passes. Debug and optimized static-CRT test execution
both pass 14 tests (13 checkpoint tests plus one real SHA module overflow test),
with overflow checks enabled and one codegen/build job.

Coverage includes 14 complete Core raw blocks (genesis on mainnet/testnet/Signet/
regtest and a nine-transaction Core Merkle fixture), 33 partial-tree/merkleblock
vectors, four witness transactions, all masks on 1–7 leaves, mutation at real
leaf/internal pairs, CompactSize boundary counts, exact/prefix/truncated input,
unknown witness flags, wrong roots and branch shapes, padding compatibility,
aggregate/per-transaction bounds, compact target vectors and 4,000 deterministic
malformed-input panic checks. Genesis expected hashes are independently sourced
from Core's chainparams; transaction/witness hashes use unchanged Core Python
codec bodies plus Python hashlib.

Independent differential result: **8,304 comparisons pass**, seed `0xB10C37`:
14 primary blocks, 256 random headers, 77 roots, 228 inclusion branches, 462
partial builds/decodes, 66 mutation cases, 2,560 target boundaries, 4,096 random
targets, 512 target encodes and 33 complete primary merkleblock payloads. Exact
reference SHA-256 and fixture counts are saved in the evidence JSON. Reference
AST adaptation removes only unused utility imports/an unrelated assertion;
block/transaction/partial-tree codec bodies are unmodified and no utility stub
is supplied. Source/fixture hashes are checked before and after Rust verification.
Tracked text-fixture checks normalize CRLF to canonical LF Git content; ignored
downloaded reference hashes remain byte-exact.

Evidence directory:
`C:/Users/user/OneDrive/Documents/ChatGPT/MagicalCryptoWallet/.artifacts/mcw-bitcoin-block/.artifacts/bitcoin-block-evidence/`
contains `verification.json`, `differential.json`, `debug.log`, `optimized.log`,
`runtime-imports.txt`, hash-pinned primary sources, and actual-source test harnesses.
Commands:

    ./mcw/tests/bitcoin_block_verify.ps1 -ReferenceDirectory <absolute ignored primary reference directory>
    python mcw/tests/bitcoin_block_inventory.py --revision HEAD

The script holds one of the two exclusive FileShare.None build slots, requires
2 GiB free memory, compiles sequentially with one codegen job and installs no
toolchain. Once QR registers the module in the existing single mcw package:

    cargo test --manifest-path mcw/Cargo.toml --locked --offline --test bitcoin_block_conformance

Windows x64 executed. Static test-binary imports are only
api-ms-win-core-synch-l1-2-0.dll, bcryptprimitives.dll, KERNEL32.dll, ntdll.dll and
USERENV.dll (Rust std/test harness imports, not third-party crypto/block code).
Linux x64/ARM64 and macOS x64/ARM64 standard libraries are absent in the shared
Windows toolchain; compilation/native execution is explicitly unverified there.
Those four platform checks and final shipping executable/package import audits
remain acceptance work; portable source alone does not prove them.

## Retained production callers and dependencies

Immutable caller snapshot: `748a961c78980c42bba293ff7ad1b9ca696566ec` in
`inventory.json` and `managed_callers.tsv`: 145 block/header/Merkle-symbol lines
in 27 files, 451 direct NBitcoin import lines in 381 files, and 62 package/version/
lock matches in 19 files. These are exact mechanical-pattern counts, including
application/coordinator/test/tool roles; implicit/global type usages remain.
No direct managed PartialMerkleTree/MerkleBlock caller exists in this snapshot.
The partial format is retained for Bitcoin filtered-peer compatibility, without
claiming a currently migrated BIP37 production path.

| Retained flow | Codec mapping and substantive work remaining |
| --- | --- |
| Wallets/FileSystemBlockRepository Block.Load/ToBytes/GetHash | Block exact decode/serialize + header hash; file/cache/pruning behavior and synthetic compatibility cutover remain. |
| Extensions/NBitcoinExtensions.DownloadBlockAsync and NodesManagement/P2pNodeClient | Actual Rust P2P messages/handshake/block retrieval, header/root/witness/structural policy and error cleanup are still needed; this codec does not replace Block.Check. |
| BitcoinP2p/BlockHeadersChainBehavior, ConcurrentChain and Client/Global header persistence | Header proof/difficulty/chain/reorg rules, explicit authoritative state boundary and production adapter remain; no storage-format change is authorized. |
| BitcoinP2p/CompactFilterBehavior and FilterSynchronizationState | Reuse published compact_filters; migrate range/anchor/checkpoint/filter verification/retry/reorg logic and peer synchronization. |
| Wallets/Wallet, Synchronizer and FilterProcessor/BlockFilterIterator | Retained managed wallet/filter state, transaction processing and event ordering must consume the verified native backend through a controlled boundary. |
| BitcoinRpc RPC/header uses | Coordinator/tool/transitional callers remain; do not reactivate removed client Bitcoin Core to satisfy this migration. |

NBitcoin 10.0.13 and NBitcoin.Secp256k1 3.1.6 remain in central versions, direct
projects and locked/transitive consumers. Other keys/curve/scripts/PSBT/RPC/P2P/
transaction consumers remain separate retained responsibilities. NBitcoin's managed
JSON/logging transitive dependencies remain; Nito.AsyncEx/Rx/Tor and other retained
managed synchronization responsibilities are not linked by this Rust codec and
are not removed by publishing it. Only when all relevant callers, locks and
packaged references are gone may an upstream package be marked removed.

## Bounded integration proposal and scope correction

Reserve `0x0E00–0x0EFF`; proposed checkpoint operations: 0x0E00 header parse/hash,
0x0E01 block framing/sizes/txids, 0x0E02 root/mutation, 0x0E03 proof build,
0x0E04 proof verify, 0x0E05 partial parse/extract/serialize, 0x0E06 partial build,
0x0E07 merkleblock payload, 0x0E08 compact target interpretation/encoding.
These are domain/API proposals, not implemented host operations. The former
0x0E20 peer-service proposal is stopped and must not be registered. QR owns frame dispatch,
module registration/manifests, shared managed bridge and platform/lifecycle/
packaging changes. Large blocks cannot fit a smaller bridge frame: design a
bounded streaming/handle path or reject oversized frames before allocating;
never truncate or silently fall back to a managed implementation.

The human scope correction revoked the full sync, peer, Tor, storage-engine,
wallet-cryptography, transaction-engine and script migrations. No managed sync
or P2P caller was edited in this workstream. The proposed ownership of
BitcoinP2p/{BlockHeadersChainBehavior,CompactFilterBehavior,
FilterSynchronizationState}.cs and Services/NodesManagement/P2pConnectionManager.cs
is withdrawn. The uncommitted `sync_service/{mod,math,chain,protocol}.rs` draft
and its test scripts remain preserved in `.artifacts/mcw-bitcoin-block-handoff`.
Its ignored `bitcoin-block-sync-evidence/preserved-scope.json` records source
hashes and the incomplete Clippy verification. It is not production code and
must not be included in host registration or a cutover.

A bounded candidate is exact 80-byte header hashing for the existing block
cache filenames, plus read identity verification against the requested hash.
`Wallets/FileSystemBlockRepository.cs` still uses NBitcoin Block mapping and
retains its existing storage format. This candidate requires narrow caller
ownership coordination, a typed native service leaf, actual host dispatch and
synthetic cache compatibility/error tests before a production replacement can
be claimed. No package retirement, whole sync replacement or changed consensus
validation is implied. In particular, `Block.Check` remains unchanged.

The coordinator alone dispatches incorporation when QR is idle. A machine handoff
record identifies this component as ready while bounded caller integration
awaits assignment; it is not a dependency-removal or workstream-completion claim.
