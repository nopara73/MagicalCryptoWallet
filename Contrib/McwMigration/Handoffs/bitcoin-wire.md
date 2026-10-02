# Bitcoin transaction wire codec handoff

Status: ready for integration, 2026-10-02 (Asia/Singapore). This is a verified
portable component of the future **one mcw executable**. Production callers,
bridge operations, native packaging, and dependency removal are not yet wired.
This document does not claim a production wallet release or NBitcoin removal.

Worker: `bitcoin-wire`, Codex thread `01a0fc2a-71e2-7f13-bff0-a34eee3e44e7`.
Integrator: `01a0fbf5-89e2-7e90-9b98-50e3ff9bb5bc` ("List all dependencies").
Coordinator: `01a0fc1e-7c20-76d3-bf81-cb1f68c9adb7`.

## Published implementation and ownership

Implementation commit: `d554088833b13446741310d8875abeb3e8a9eddc`, verified on
`origin/master` by `git ls-remote` after a normal push. It contains only:

- `mcw/src/bitcoin_wire.rs`
- `mcw/tests/bitcoin_wire_conformance.rs`
- `mcw/tests/bitcoin_wire_reference.py`
- `mcw/tests/bitcoin_wire_verify.ps1`
- `mcw/tests/bitcoin_wire_fixtures/reference.tsv`
- `mcw/tests/bitcoin_wire_fixtures/manifest.json`
- `mcw/tests/bitcoin_wire_fixtures/.gitattributes`

The subsequent handoff commit contains this document only; its exact ID is the
`commit` in the shared, ignored
`.artifacts/mcw-coordination/handoffs/bitcoin-wire.json`. The implementation ID
above is not a claim of ownership of other workers' sources. No Cargo manifest,
`lib.rs`, host/command/main/bridge, managed adapter, packaging, launch path,
shared ledger, wallet file, key, or platform implementation was edited.

Working/evidence checkout:
`C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-bitcoin-wire`.
Publication operations held exclusive `FileShare.None` access to the shared
`.artifacts/git-publish.lock`, checked shared and worker indexes were empty, and
staged only the owned paths. No reset, stash, force push, shared branch switch,
or pull request was used. A stale configured GitHub CLI credential-helper path
required an authenticated, command-scoped helper override; shared Git settings
were not changed.

## Portable API

The integrator declares `bitcoin_encoding` and `bitcoin_wire` modules in the
existing mcw crate. There is no second Cargo package or shipping executable.
`bitcoin_wire` forbids unsafe code and has no C#, Avalonia, IPC, OS-handle, native
library, or third-party crate dependency.

| API | Contract |
| --- | --- |
| `Transaction::decode(bytes, &Limits)` | Decode one exact witness-aware transaction; reject extra bytes. |
| `Transaction::decode_legacy(bytes, &Limits)` | Decode explicit legacy format, including zero-input and/or zero-output unsigned transactions. |
| `Transaction::decode_prefix(bytes, &Limits, DecodeMode)` | Decode one prefix and return `(Transaction, consumed_bytes)`; caller owns subsequent bytes. |
| `Transaction::serialize(&Limits)` | Canonical transaction bytes; include marker `00` / flag `01` only when a witness stack is nonempty. |
| `Transaction::serialize_legacy(&Limits)` | Canonical stripped bytes; deliberately omit witnesses, while still checking whole-object resource budgets. |
| `Transaction::{txid,wtxid}(&Limits)` | SHA256d of stripped/full canonical bytes through the real first-party encoding module. |
| `Transaction::has_witness()` | True for any nonempty stack, including a stack containing only one empty byte vector. |
| `Transaction::sizes(&Limits)` | Checked stripped/total sizes, weight, and virtual size; does not enforce consensus weight policy. |
| `decode_output` / `serialize_output` | Exact standalone `TxOut` format, useful for PSBT witness UTXOs. |
| `decode_witness` / `serialize_witness` | Exact standalone scriptWitness stack format, useful for PSBT final witness fields. |
| `decode_compact_size(bytes)` | Canonical CompactSize prefix; returns `(u64, consumed_bytes)`, permits following bytes. |
| `write_compact_size(value, &mut Vec<u8>)` | Append shortest encoding; reserve failure leaves the existing output intact. |
| `compact_size_len(value)` | Encoded length: 1, 3, 5, or 9 bytes. |

All fallible APIs return `Result<_, bitcoin_wire::Error>`. Errors distinguish
truncation with byte offset, noncanonical CompactSize, unknown witness flags,
superfluous witness encoding, trailing bytes, resource limits, arithmetic
overflow, allocation failure, and an internal hashing error. No input trimming,
repair, partial success, execution, or implicit alternate-format retry occurs.

Data model:

```rust
Transaction { version: i32, inputs: Vec<TxIn>, outputs: Vec<TxOut>, lock_time: u32 }
TxIn { previous_output: OutPoint, script_sig: Vec<u8>, sequence: u32,
       witness: Vec<Vec<u8>> }
OutPoint { txid: [u8; 32], vout: u32 }
TxOut { value: i64, script_pubkey: Vec<u8> }
TxId([u8; 32]) // raw digest; Display reverses bytes for transaction-id text
```

All version bits, locktime, outpoint index, sequence bits, signed output values,
and unknown script/witness bytes survive exactly. Negative or excessive monetary
values are deliberately retained; successful wire parsing does **not** imply
MoneyRange, transaction validity, script validity, signature validity, ownership,
or spendability. No signing, key handling, sighash, block/header validation,
fee policy, or contextual consensus check is provided.

### Witness and legacy edge cases

Unknown nonzero flag bits are rejected immediately. Flag `01` with every input's
witness stack empty is rejected. An empty item inside a nonempty stack is valid
wire data. There is exactly one witness stack per input; no separate witness-stack
count is guessed. Without witness, `txid == wtxid`. The transaction-hash method
does not replace a coinbase wtxid with the special all-zero witness-commitment leaf.

Witness-aware parsing follows Bitcoin Core's empty-vin marker interpretation:
`version / 00 / 00 / locktime` is an empty legacy transaction. A legacy transaction
with zero inputs and nonzero outputs is ambiguous in this mode, so PSBT's global
unsigned transaction **must** use `decode_legacy`. This explicit mode supports
the BIP174 zero-input/zero-output examples. The PSBT worker has independently
tested the actual module with its BIP174/BIP370 examples.

Canonical accepted bytes round trip exactly in their selected mode. Stripped
serialization intentionally discards witness. `decode_prefix` is for deliberate
stream/block framing; it must never replace exact decoding at an RPC, file, or
PSBT field boundary.

### Bounds and allocations

Default `Limits` are application resource limits:

| Resource | Default ceiling |
| --- | ---: |
| Consumed transaction / standalone container bytes | 4,000,000 |
| Inputs | 100,000 |
| Outputs | 100,000 |
| Individual script bytes | 4,000,000 |
| Witness items per input | 100,000 |
| Witness items across all inputs | 100,000 |
| Individual witness item bytes | 4,000,000 |
| Copied script/witness payload bytes across the object | 4,000,000 |
| Logical decoded allocation bytes | 32,000,000 |

Callers may tighten these ceilings. They are not consensus or relay-policy rules.
CompactSize uses the full `u64` domain; conversion, addition and multiplication
are checked. Impossible count declarations are checked against the available
wire bytes and transaction byte budget before vector allocation. `try_reserve`
or `try_reserve_exact` handles allocation failure. The memory budget includes
transaction/input/output structs, witness vector elements (even empty items),
and copied payload. It excludes caller-owned input/object memory, allocator
bookkeeping, and the separately byte-bounded serialized output buffer. It is not
a process RSS promise. Serialization/hashing check all object budgets first.
Parsing is linear in bounded bytes/items and never recursively interprets scripts.

## Verification and independent reference evidence

Compiler: Rust `1.99.0 (b940084d7 2026-09-28)`, edition 2024,
`x86_64-pc-windows-msvc`. Both debug and optimized builds used overflow checks,
`-D warnings`, and one test thread. **21 wire tests plus the encoding module's
SHA256 length-state test passed (22 total) in each build.** The combined tests
were rerun against the encoding worker's final, actual implementation, not a
mock or duplicate hash implementation:

- Wire source SHA256: `27daf041141305000d8329e2cbc46279d5c66f21232cc58f4bdbe53793420f1a`
- Final encoding source SHA256: `4e59e9210317f6c75f1196d22892202b3c72f62adcf9d05be88c4c3c66925ec0`
- Conformance test source SHA256: `f7a73e813f93eb48da1badc9de8ef347510abcc97ff196d0698b1a2f602967c3`
- Reference fixture SHA256: `f6d38678b57c88bc372a794aa8d2ff6001ebcebd5b6ef66ba73bd404cbb9f543`

`clippy-driver --edition=2024 --test -D warnings --emit=metadata` on the combined
actual-source harness passed. `rustfmt --edition 2024 --check` passed for both
owned Rust files. `git diff --check` / staged whitespace checks passed.

The checked-in corpus contains **360** independently generated cases:

- 120 published Bitcoin Core v29.0 `tx_valid.json` transactions.
- 93 published `tx_invalid.json` transactions (their consensus/script-invalid
  classification does not make their binary format malformed).
- Six distinct retained `AllTransactionStoreTests.cs` literals used by existing
  application tests; no live wallet or user database was read.
- 141 deterministic synthetic cases, including signed amount extremes, version
  sign bits, zero-input/output legacy cases, and CompactSize boundaries in counts,
  scripts, and witness stacks.

The generator imports the **unmodified** pinned Bitcoin Core `messages.py` and
its original stdlib-only utility imports. It uses Core's own parser/serializers,
hash calculation (Python's independent `hashlib`), and sizes. Every reference
source download has a verified SHA256 in `manifest.json`; no reference row was
skipped. Every Rust transaction matches the original bytes, stripped bytes,
txid/wtxid, version bits, locktime, counts, weight/vsize, and an independent
fingerprint covering every outpoint, script, sequence, amount, and witness item.
The fixture `.gitattributes` preserves the content hashes on Windows checkouts.

Additional tests cover all truncation offsets in representative legacy/witness
transactions and standalone containers; noncanonical CompactSize in every
container; flags `02` through `ff`; all-empty witness records; one empty witness
item; trailing bytes vs prefix framing; every resource ceiling; aggregate witness
metadata/payload budgets; huge declared `u64` lengths; the exact 4,000,000-byte
limit and one byte above it; 10,000 deterministic malformed buffers; and every
single-bit mutation of a witnessed sample. Accepted fuzz/mutation cases preserve
their exact bytes. Hash tests also prove witness changes leave txid stable while
changing wtxid, and scriptSig changes affect txid.

Reproduce from this worker checkout:

```powershell
python mcw/tests/bitcoin_wire_reference.py
& mcw/tests/bitcoin_wire_verify.ps1
```

The Windows verifier compiles an ignored harness against the actual encoding
worker source if that source is not yet in this checkout. It takes a shared build
slot, checks at least 2 GiB free memory, uses single-job execution, checks source
hashes remain stable, and writes `.artifacts/bitcoin-wire/verification.json` plus
debug/optimized logs. Reference generation downloads only into that ignored
workspace using background HTTP. It neither starts a node nor signs or pays.

After host integration, run the same checked-in conformance suite using the
existing mcw Cargo package, e.g. `cargo test --offline --manifest-path mcw/Cargo.toml
--test bitcoin_wire_conformance -- --test-threads=1`, with the integrator's
production native-runtime build configuration and one shared build slot.

Primary specifications/reference sources:
[BIP144](https://github.com/bitcoin/bips/blob/master/bip-0144.mediawiki),
[BIP141](https://github.com/bitcoin/bips/blob/master/bip-0141.mediawiki),
[Bitcoin Core v29.0 transaction serialization](https://github.com/bitcoin/bitcoin/blob/v29.0/src/primitives/transaction.h),
[CompactSize implementation](https://github.com/bitcoin/bitcoin/blob/v29.0/src/serialize.h),
[Core reference transactions](https://github.com/bitcoin/bitcoin/blob/v29.0/src/test/data/tx_valid.json),
[Core test framework](https://github.com/bitcoin/bitcoin/blob/v29.0/test/functional/test_framework/messages.py).
Bitcoin Core test data and reference tooling are MIT licensed; they are reference
test material, not copied implementation or production dependencies.

## Dependency and runtime audit

The only non-stdlib import of `bitcoin_wire.rs` is the sibling first-party
`crate::bitcoin_encoding`. Hash API used:
`double_sha256(&[u8]) -> Result<[u8; 32], bitcoin_encoding::Error>`.
The hashing source was read and actually compiled together with this module.
No external crypto/encoding crate or separate worker implementation was copied.
The encoding worker published that exact source in
`6365f3244d23b801c0bb36581967caad8aae968e`; its publication is a dependency handoff,
not work attributed to the bitcoin-wire worker.

The live integrator manifest snapshot at
`.artifacts/mcw-host-qr/mcw/Cargo.toml` has empty normal/dev/build dependency
sections; its lockfile contains only one package, `mcw`. Snapshot digests:
manifest `b85dff5d66b164528e31e4fa4a009ac25ed73ea42b7ad3aae05bbf029b66e4c9`,
lock `f8ffe397a2a9658af984f6488d19076a7de682c37b62d04fdb0eed3029b2d542`.
No Cargo file was created or edited by this worker. The ignored
`.artifacts/bitcoin-wire/dependency-audit.json` records that snapshot.

Only the Windows test harness ran here. It used the already installed MSVC/SDK
linker with `-C target-feature=+crt-static` because the normal x64 CRT import-library
path was absent. That test executable remains ignored and is not shipped. This
is **not** a production native-runtime dependency audit or five-platform package
claim. Linux x64/ARM64 and macOS x64/ARM64 execution/cross-build evidence, and all
production runtime-linkage checks, remain integrator acceptance checks. No SDK,
runtime, crate, installer, or companion executable was installed for this module.

## Retained application callers and dependencies

Caller graph baseline: implementation commit above (managed sources unchanged
from `82127991068522210cdcf77080dc9b819502e486`). The following live production
paths still use NBitcoin's transaction binary/hex/hash responsibility and must be
routed by the integrator before this responsibility can be reported migrated:

| Path | Retained behavior |
| --- | --- |
| `MagicalCryptoWallet.Client/Rpc/MagicalCryptoWalletJsonRpcService.cs` | `Transaction.Parse` at 373/424; transaction hex at 244/272/399/417; RPC serializers also expose transaction/hash objects. |
| `MagicalCryptoWallet/Stores/TransactionSqliteStorage.cs` | Binary `ToBytes` at 187; `Transaction.Load` at 416; txids stored in raw little-endian/digest order. |
| `MagicalCryptoWallet.Fluent/Models/TransactionBroadcasterModel.cs` | Raw transaction import, `Transaction.Parse` at 24. |
| `MagicalCryptoWallet.Fluent/Helpers/TransactionHelpers.cs` | Import/parse helper at 121. |
| `MagicalCryptoWallet/Blockchain/TransactionBroadcasting/TransactionBroadcaster.cs` | Broadcast transaction hex at 104. |
| `MagicalCryptoWallet/Blockchain/Transactions/TransactionSummary.cs` | Transaction hex at 22. |
| `MagicalCryptoWallet/Exceptions/InvalidTxException.cs` | Transaction hex in diagnostics at 25. |
| `MagicalCryptoWallet/Blockchain/Transactions/TransactionBroadcastEntry.cs` | Witness transaction hash matching at 59. |
| `MagicalCryptoWallet/Blockchain/Mempool/MempoolService.cs` | Witness transaction hash comparison at 33. |
| `MagicalCryptoWallet/Blockchain/Transactions/SmartTransaction.cs` | Delegated txid at 190 and transaction identity throughout wallet metadata. |
| `MagicalCryptoWallet/Rpc/JsonConverters/SmartTransactionJsonConverter.cs` | Managed serializer delegates the NBitcoin `Transaction` object at 17. |
| `MagicalCryptoWallet/Serialization/Bitcoin.cs` | Binary outpoints at 22/94-100 and scriptWitness at 48-49/91-92, plus script/value objects. |
| `MagicalCryptoWallet/Serialization/Coordination.cs` | Witness/state and transaction-signature request serialization/decoding via the preceding helpers. |
| `MagicalCryptoWallet/Extensions/NBitcoinExtensions.cs` | Generic exact binary `IBitcoinSerializable` decoding at 355-366; includes unrelated serialized types. |
| `MagicalCryptoWallet/Crypto/Bip322Signature.cs` | BitcoinStream/scriptWitness serialization at 24-33; signing/verification responsibilities remain outside this module. |

`Crypto/ProofBody.cs`, `Crypto/OwnershipProof.cs`, and
`Crypto/OwnershipIdentifier.cs` also retain BitcoinStream for non-transaction
ownership-proof formats. WabiSabi `SigningState`, coordinator `Round`,
`TransactionSignaturesRequest`, and `ArenaClient` retain WitScript/signature
objects. The wallet, transaction builder/factory, PSBT, script policy, keys,
secp256k1 signing, peer/network objects, filters, addresses, proofs, and CoinJoin
continue to have unrelated NBitcoin responsibilities. The inspected core/client/
Fluent source graph has 189 files importing/referring to NBitcoin. Therefore
**NBitcoin 10.0.13 and NBitcoin.Secp256k1 3.1.6 remain retained**, as do the existing
transitional managed app and its dependencies; none were removed in this work.

Refresh the caller graph when integrating, since other workers also publish:

```powershell
rg -n 'Transaction\.(Parse|Load)|\.Transaction\.To(Bytes|Hex)|GetWitHash|BitcoinStream' MagicalCryptoWallet MagicalCryptoWallet.Client MagicalCryptoWallet.Fluent -g '*.cs'
rg -n 'WitScript|Outpoint\(|SmartTransactionJsonConverter' MagicalCryptoWallet MagicalCryptoWallet.Client -g '*.cs'
rg -l '\busing NBitcoin|\bNBitcoin\.' MagicalCryptoWallet MagicalCryptoWallet.Client MagicalCryptoWallet.Fluent -g '*.cs'
```

## Bridge reservation and integration acceptance

Reserved proposal: **`0x0500–0x05FF`**, implementation/wiring exclusively owned by
the QR integrator. Suggested operations: exact structural decode/roundtrip,
stripped serialization, txid/wtxid plus sizes, and exact TxOut/scriptWitness or
CompactSize handling. The domain module itself knows nothing about bridge frames
or managed object formats. Bridge layout/error mapping must follow the existing
mcw host contract; expose limits without leaking native handles into this module.

Before integration/removal acceptance:

1. Incorporate the implementation commit and the final verified first-party
   encoding module; declare modules in the existing mcw crate only. Rerun current
   combined conformance and the PSBT worker's real-source suite.
2. Keep legacy-vs-witness mode explicit at every boundary. PSBT unsigned fields
   use legacy mode; peer/broadcast/raw-import/storage bytes use the appropriate
   witness-aware exact mode. Do not accept trailing bytes or unknown flags.
3. Preserve signed values, unknown scripts/witness, sequence/version bits,
   locktime, raw BLOB/outpoint digest order, and reversed transaction-id display.
   The wire codec must not replace signing, consensus, fees, script analysis, or
   wallet policy gates.
4. Route and verify every applicable retained caller above, including persistence,
   imports, broadcasting, hashes, RPC serialization and witness containers. Use
   synthetic fixtures/data for integration and never migrate live wallet state
   merely to demonstrate the codec.
5. Run current-source native-runtime tests/builds for Windows x64, Linux x64/ARM64,
   and macOS x64/ARM64. Audit final Cargo tree/lock and binary imports/linkage;
   static test CRT evidence is not acceptable production evidence.
6. Keep NBitcoin marked retained until every unrelated caller is independently
   migrated by its owner. Update the shared migration ledger only as the
   integrator; do not interpret this ready handoff as completed package removal.

The ignored machine handoff is coordination evidence, not an integration request.
The coordinator owns dispatch to the QR integrator when that chat is idle.
