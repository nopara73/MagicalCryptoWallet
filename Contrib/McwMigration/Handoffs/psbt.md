# PSBT format handoff

The first-party PSBT container implementation is ready for integration into the single `mcw` executable. Its scope is lossless BIP174 v0/BIP370 v2 framing, typed field shapes, transport, and unsigned transaction construction. NBitcoin remains required by the managed wallet's builders, signing, extraction, keys, scripts, and other callers. This handoff does not establish production wallet or release readiness.

## Publication and ownership

- Implementation commit: `211afbb71426e31d9ebc3a088674abe020160e5f` (published directly to `origin/master`; no PR).
- Worker: `psbt`, local thread `01a0fc2a-8948-7ab0-85aa-2cfc4768271f`.
- Module owner paths: `mcw/src/psbt.rs`, `mcw/tests/psbt_*`, and this document.
- Integration ownership stays with thread `01a0fbf5-89e2-7e90-9b98-50e3ff9bb5bc`: Cargo manifests, root declarations, application lifecycle, commands/bridge, managed adapters, packaging, launch paths, and migration ledger.
- Published internal dependencies tested: `bitcoin_wire` commit `d554088833b13446741310d8875abeb3e8a9eddc`; `bitcoin_encoding` commit `6365f3244d23b801c0bb36581967caad8aae968e`. No mock transaction parser or hash implementation is used.
- All staging, commits, pushes, and remote reconciliation hold exclusive `FileShare.None` access to the shared `.artifacts/git-publish.lock`. Only the eight listed implementation/test files and this owned handoff are staged. Other agents' indexes, changes, processes, and data are preserved.

Implementation/test files:

```text
mcw/src/psbt.rs
mcw/tests/psbt_conformance.rs
mcw/tests/psbt_vectors.tsv
mcw/tests/psbt_vectors.py
mcw/tests/psbt_vectors.md
mcw/tests/psbt_managed_vectors.tsv
mcw/tests/psbt_managed_reference.ps1
mcw/tests/psbt_verify.ps1
```

## Format behavior

The parser checks the five magic bytes, canonical CompactSize encodings for lengths/types/counts, exact value lengths, map terminators/counts, trailing bytes, and duplicate full keys in each map. Key uniqueness includes all key data, so different public keys of the same type remain legal. Map order, raw keys/values, unknown extensions, and proprietary identifiers/subtypes/subkeys are preserved. Every accepted binary container serializes to exactly its original bytes. There is no sorting, discarding, network lookup, platform handle, IPC, managed dependency, or OS-specific format branch.

Version defaults to zero only when the version field is absent. An explicit zero version field is preserved. Only versions zero and two are supported; other versions fail closed. V0 requires exactly one global unsigned transaction, decoded with the wire codec's explicit legacy mode and empty scriptSigs/witnesses. Zero-input and zero-output legacy transactions are permitted by BIP174. V0 rejects all BIP370 fields in their corresponding scopes. V2 excludes global unsigned transaction and requires version, transaction version, input/output counts, each input's previous TXID/output index, and each output's amount/script. It retains undefined transaction-modifiable flag bits, as the BIP370 valid vectors require.

Known BIP174/BIP370 fields are checked for key-data shape and field length. Pubkeys accept compressed 33-byte encodings or uncompressed 65-byte encodings, with the appropriate prefix; xpubs require 78 bytes, compressed key shape, and matching origin depth. Origins require a four-byte fingerprint and complete little-endian path indexes. Partial ECDSA signatures have strict DER encoding plus a sighash byte. Hash-key lengths, v2 TXIDs, integer sizes, count encodings, locktime ranges, proprietary CompactSize identifier/subtype encodings, transaction outputs, previous transactions, and final witness stack framing are checked. `NON_WITNESS_UTXO`, `WITNESS_UTXO`, and `FINAL_SCRIPTWITNESS` use the actual first-party wire decoder.

Amounts retain signed 64-bit wire values, including values invalid for consensus. Curve membership, signature verification, acceptable sighash policy, preimage/hash equality, UTXO commitments, script execution, money range, ownership, and spendability are outside the format layer. BIP371 and other extensions are preserved as unknown records and do not receive extension-specific validation here. A successful parse is never signing authorization or proof of transaction validity.

V2 locktime follows the BIP370 algorithm: consider inputs that specify a requirement, choose a type supported by all those inputs, prefer height when both types work, take the maximum of that type, and otherwise use fallback locktime or zero. Incompatible height-only/time-only requirements remain parseable and lossless, but `locktime()` and transaction construction return `IncompatibleLocktimes`. Ordinary unsigned construction uses specified sequences or `0xffffffff`; the identifier preimage sets all v2 sequences to **zero**, as BIP370 requires. Neither method finalizes or extracts a signed transaction or hashes the identifier preimage.

## Portable API

All public operations return `Result<T, psbt::Error>` where they can fail. `Error` contains its kind, `Scope::Global/Input(index)/Output(index)`, and an absolute container byte offset when known; field-local positions are not reported as absolute container offsets.

```rust
Psbt::parse(bytes: &[u8], limits: Limits) -> Result<Psbt, Error>
Psbt::from_base64(text: &str, limits: Limits) -> Result<Psbt, Error>
Psbt::parse_text(text: &str, limits: Limits) -> Result<Psbt, Error>
Psbt::from_maps(global: Map, inputs: Vec<Map>, outputs: Vec<Map>, limits: Limits)
    -> Result<Psbt, Error>

psbt.version() -> Version
psbt.global() -> &Map
psbt.inputs() -> &[Map]
psbt.outputs() -> &[Map]
psbt.limits() -> Limits
psbt.serialized_len() -> usize
psbt.serialize() -> Result<Vec<u8>, Error>
psbt.to_base64() -> Result<String, Error>
psbt.locktime() -> Result<u32, Error>
psbt.unsigned_transaction() -> Result<Vec<u8>, Error>
psbt.identifier_transaction() -> Result<Vec<u8>, Error>

Record::new(key_type: u64, key_data: &[u8], value: &[u8]) -> Result<Record, Error>
Map::new(records: Vec<Record>) -> Result<Map, Error>
map.records() -> &[Record]
map.get(key_type: u64, key_data: &[u8]) -> Option<&Record>
map.singleton(key_type: u64) -> Option<&[u8]>
map.with_record(record: Record) -> Map
map.without_record(key_type: u64, key_data: &[u8]) -> Map
record.key()/key_data()/value() -> &[u8]
record.key_type() -> u64
record.field(scope: Scope) -> Result<Field<'_>, Error>
```

`Field` supplies borrowed typed views for every recognized BIP174/BIP370 field, including `KeyOrigin` (fingerprint and path iterator) and `Proprietary` (identifier/subtype/key data/value). The `Psbt`/`Map`/`Record` bytes are immutable; map edits make new maps and must go through `Psbt::from_maps` for scope/version/resource validation. Builder helper arguments are already allocated caller data; use the bounded parse functions for untrusted wire/text input.

`from_base64` accepts canonical, padded RFC4648 standard Base64, rejects whitespace, the URL-safe alphabet, nonzero padding bits, missing/extra padding, and non-ASCII bytes, and checks decoded size before allocating. `parse_text` trims **ASCII** outer whitespace and accepts that Base64 or case-insensitive hex starting with PSBT magic; internal whitespace remains rejected. The latter matches the former binary/hex/Base64 import boundary while making canonical transport behavior explicit. PSBT itself has no network selector; any network policy belongs to the wallet/adapter.

## Bounds

| Limit | Default |
| --- | ---: |
| Total binary bytes | 16 MiB |
| Maps, including global | 20,001 |
| Total records | 100,000 |
| Records per map | 4,096 |
| Bytes per key | 16,384 |
| Bytes per value | 4 MiB |

Counts and lengths use checked arithmetic and are tested before allocating from them. At least one terminator byte per input/output map must remain before reserving map vectors. Key/value copies and serialization use fallible `try_reserve`; duplicate-key detection uses a bounded ordered set. Embedded transaction input/output counts are capped by the PSBT map ceiling; wire byte/script/item/payload ceilings are the smaller of PSBT's value and total byte limits. The wire decoder also retains its own bounded witness counts and decoded-memory budget. Limits are application resource policy, not consensus limits.

## Reserved bridge proposal

The following historical codec-only proposal was never incorporated. The current
bounded live metadata assignment supersedes these proposed meanings; the exact
`0x0600-0x0606` typed operations and session transfer are documented in
[psbt-metadata.md](psbt-metadata.md). It preserves the existing 1 MiB bridge
ceiling and retains the managed builder and signer.

The reserved range is `0x0600-0x06FF`. This worker implements no bridge or shipping executable. Suggested operations for the host owner:

| Operation | Responsibility |
| --- | --- |
| `0x0600` | Bounded binary PSBT validation and version/map-count summary |
| `0x0601` | Lossless binary parse/serialize |
| `0x0602` | Canonical Base64 to binary |
| `0x0603` | Validated binary to canonical Base64 |
| `0x0604` | Legacy unsigned transaction bytes |
| `0x0605` | V0 identity bytes / v2 zero-sequence identifier preimage |

Retain separate failure outcomes for malformed/unsupported containers, resource limits, and incompatible locktime requirements. The adapter should not turn validation into a signing/finalization claim or log raw container contents.

## Current managed callers and dependency retention

The initial remote caller snapshot was `82127991068522210cdcf77080dc9b819502e486`; the final publication parent is `748a961c78980c42bba293ff7ad1b9ca696566ec`. Current production PSBT callers are:

| Path / method | Existing NBitcoin responsibility | Handoff boundary |
| --- | --- | --- |
| `MagicalCryptoWallet/Blockchain/Transactions/TransactionFactory.cs`, `BuildTransaction` | `BuildPSBT`, input/output inspection, fee/vsize calculation, metadata, `SignPSBT`, finalization/extraction | `Psbt::from_maps`/typed maps cover the container; builders, fee policy and signing remain separate work |
| `MagicalCryptoWallet/Blockchain/TransactionBuilding/BuildTransactionResult.cs` | Carries the NBitcoin `PSBT` instance | Future mcw-owned container can use `Psbt`; managed adapter/type migration belongs to integrator |
| `MagicalCryptoWallet/Wallets/WalletAuthorization.cs`, `Sign` | Clone, software signing, finalize, extract | Immutable map rebuild replaces container cloning/editing; signing/authorization/finalization stay retained |
| `MagicalCryptoWallet/Extensions/NBitcoinExtensions.cs`, `ExtractSmartTransaction`, `GetInputScriptPubKeyType`, `AddKeyPaths`, `AddKeyPath`, `AddPrevTxs` | Extraction, script policy, origins and previous transactions | Typed BIP32/UTXO records cover format data; script/key/wallet semantics stay retained |
| `MagicalCryptoWallet/Blockchain/TransactionBuilding/TransactionModifierWalletExtensions.cs` | Uses `tempTx.Psbt.TryGetVirtualSize` | Wire bytes are available; estimation/signing policy is outside this module |

Managed regression callers remain in `MagicalCryptoWallet.Tests/UnitTests/WalletOperationAuthorizationTests.cs` (reviewed unsigned transaction unchanged by signing) and `MagicalCryptoWallet.Tests/UnitTests/SoftwareWalletTests.cs` (preview/signing finalization state). They are not removed or rewritten here.

Hardware-wallet removal eliminated PSBT text/file import/export from the inspected managed workflows. The standalone transaction broadcaster and raw-transaction import/paste are also removed. Do not resurrect hardware wallet, PayJoin, or removed import/export flows to manufacture a production caller. Re-audit the fresh caller graph before integrating. NBitcoin 10.0.13 is still a direct dependency in the managed core lockfile; 188 C# files in the inspected core/client/Fluent directories reference NBitcoin, and test/daemon/coordinator uses remain as well. This module does **not** justify removing the package.

Inventory commands:

```powershell
rg -n '\bPSBT\b|\.Psbt\b|PSBTInput|PSBTOutput|BuildPSBT|SignPSBT' -g '*.cs' -g '!**/obj/**' -g '!**/bin/**'
rg -n 'PSBT|Psbt' MagicalCryptoWallet.Fluent/Helpers/TransactionHelpers.cs MagicalCryptoWallet.Fluent/Models/TransactionAuthorizationInfo.cs MagicalCryptoWallet.Fluent/Helpers/FileDialogHelper.cs
rg -l '\bNBitcoin\b' MagicalCryptoWallet MagicalCryptoWallet.Client MagicalCryptoWallet.Fluent -g '*.cs'
```

## Verification and compatibility evidence

Reference specifications are pinned [BIP174](https://github.com/bitcoin/bips/blob/3a10b5b5f0a7586df8928d580a3009744ebb2079/bip-0174.mediawiki) and [BIP370](https://github.com/bitcoin/bips/blob/3a10b5b5f0a7586df8928d580a3009744ebb2079/bip-0370.mediawiki), bitcoin/bips commit `3a10b5b5f0a7586df8928d580a3009744ebb2079`. Their source SHA256 values are `f2a8e1a9c9e31cc7f607b3c7e2419c63eeccc4bd5b725968cde54ab8cfa1d410` and `b2b6e9099592fa04179383f9a1fd8b201fbeef4470f4c3f1c050d25589979c57`. `psbt_vectors.md` supplies attribution, license and regeneration commands.

Results on Rust 1.99.0, edition 2024, Windows x64:

- All 94 official examples: 44 malformed containers rejected, 46 valid containers accepted and reproduced byte for byte, four signer-only failures accepted as containers. All published hex/Base64 pairs independently matched Python standard library codecs.
- All nine BIP370 locktime examples, including the unnamed additional vector and incompatible-type error. Exact unsigned and zero-sequence identifier bytes match manually independent expectations and are decoded by real `bitcoin_wire` source.
- All 47 retained NBitcoin 10.0.13 binary/Base64 exports of container-valid reference cases are parsed and reproduced byte for byte. Their hex/Base64 pairs also independently matched Python's codecs. The existing managed core lockfile pins that exact package version; its cached assembly SHA256 is `ebb7e5548fe1325514289528e67b2ee0e24b3bfecb75ed4067c44ea99a202167`.
- Every-offset truncation, deterministic mutations, duplicate keys in all scopes, typed lengths/key origins/pubkeys/DER shapes, previous transaction/TxOut/final witness validation, unknown/proprietary passthrough, CompactSize boundary sizes, excessive advertised lengths/counts, all configured limits, strict transport errors, and 10,000 bounded deterministic noise inputs passed.
- Final debug and optimized runs each pass 18 owned PSBT tests plus one encoding-module unit test (19 total), with overflow checks enabled. `rustfmt --check` and Clippy with warnings denied pass. Actual published wire/encoding source is copied byte for byte into an ignored isolated harness; no mocks or secondary Cargo package are involved.

The managed reference run uses `Network.Main` and tests a complete Load/ToBytes/ToBase64 path. It reports 48 successful import/export paths and 46 failed ones. Three otherwise valid examples do not complete that path: the testnet-xpub example requires `Network.TestNet` (verified accepted with that network), the zero-input/zero-output example loads but both exports throw `The transaction must have at least one input`, and the zero-input/two-output example fails loading. NBitcoin also accepts the BIP370 invalid required-height-locktime-zero example; this implementation rejects it as the specification requires. The Rust container is network-neutral and deliberately follows the official format in these differences.

Run without downloading any dependency:

```powershell
./mcw/tests/psbt_verify.ps1
./mcw/tests/psbt_verify.ps1 -Optimized
```

The script snapshots the real modules into its own ignored `.artifacts/psbt-validation/<run>` directory, obtains one of the two shared exclusive build slots, defers Windows compilation below 2 GiB free memory, builds one test harness with one codegen unit, and writes test logs/source hashes/evidence. `-RustBin`, `-BitcoinWireSource`, and `-BitcoinEncodingSource` can select already existing toolchain/module source paths. There is no installer or restore step. To repeat the optional transitional baseline with the existing cached assembly, run `./mcw/tests/psbt_managed_reference.ps1`; the checked-in export fixture means NBitcoin/.NET is **not** needed by the Rust tests.

Evidence directories for the final two runs: `.artifacts/mcw-psbt/.artifacts/psbt-validation/20261002-185945-122-96f171a1` and `.artifacts/mcw-psbt/.artifacts/psbt-validation/20261002-185650-290-4f25c4aa`. The original reference extraction and managed comparison live in the same worker checkout under `.artifacts/psbt-evidence`. The machine-readable ready handoff lives at shared `.artifacts/mcw-coordination/handoffs/psbt.json`, written only after publication is verified.

## Dependency and platform audit

`psbt.rs` uses `std` and the published internal `crate::bitcoin_wire` module only, forbids unsafe code, and contains no platform APIs, C#/Avalonia/IPC references, external codecs, packages, native libraries, companion executables, runtime installers, filesystem or network access. Wire hashing uses the already published internal first-party encoding module. Optional fixture preparation tools (Python/.NET/PowerShell/Git/SDK) are development tooling; the Rust tests themselves use only the Rust standard library and actual application module source.

The live QR-host `cargo metadata --locked --offline --format-version 1` audit reported one application package, `mcw`, and zero external dependencies including dev/build dependencies. This is a current source-graph audit, not a claim that the uncommitted host, managed app, packaging, or product runtime imports are already migrated. No shared manifest/declaration/bridge/adapter/packaging surface is edited here, and no second Cargo package or shipping executable is added.

Native conformance was executed on Windows x64 only. Windows x64, Linux x64/ARM64 and macOS x64/ARM64 share the same portable source; their cross-target builds, native tests and product runtime/clean-machine verification are still the integrator's release checks. No platform release is claimed from component tests.

## Integration and removal acceptance checks

1. Integrate the published wire/encoding modules and this PSBT commit into the one application, then add only the host-owned root declarations/commands/bridge wiring. Run the two PSBT verifier modes and the real Cargo-integrated PSBT tests against those exact sources.
2. Re-audit current callers and replace the surviving container responsibility at its actual application boundary without restoring removed features. Exercise review/authorization flows with synthetic data and preserve the reviewed unsigned transaction, metadata, signatures and unknown/proprietary records.
3. If a binary/hex/Base64 transport boundary is retained or explicitly reintroduced by the user, exercise both directions through that actual caller with the checked-in official and managed fixtures; surface strict whitespace/padding/version/resource behavior deliberately.
4. Keep signature/script/UTXO/amount/network/authorization checks in the proper domain layer. Containers with incompatible v2 locktime requirements must fail unsigned construction without data loss. Use zero sequences only for v2 identifier preimages.
5. Complete builds and meaningful native verification for all five supported desktop targets, then audit the one executable's runtime imports and final package for any non-OS runtime, non-first-party library, companion executable or retained format dependency.
6. Retain NBitcoin until every other live caller, signing/building/key/script responsibility, package/lockfile reference and packaged runtime asset is removed and verified. Mark standalone module readiness, production integration and package removal separately in the shared ledger.

The coordinator owns any integration request dispatch. This worker publishes the ready handoff and does not interrupt active QR-host work.
