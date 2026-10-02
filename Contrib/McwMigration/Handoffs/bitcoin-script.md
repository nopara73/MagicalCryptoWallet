# Wallet Script format and signature-hash checkpoints

State: **verified checkpoints preserved; bounded caller assignment pending**.
The human scope correction of 2026-10-02 stops the full Script/transaction
validation migration. No interpreter or validation-engine code was written.
No production caller has been migrated by these checkpoints. They do not replace
NBitcoin as a package, implement script execution, or prove spendability.

Worker: bitcoin-script, thread 01a0fc4b-dd8a-72b2-bed8-8298491abaa4.
Format implementation commit: bc4d917ec209ea0f5bb27630e4dc48b76bcc3665.
Signature-hash implementation commit: 88bd734f82862fda5b8210cb37fd112d98705d97.
Canonical LF source SHA-256:
`cbfaba9dda02462748ad036b1ce4500e433413c310f00f59f6db718e4e8e9556`.
Normal direct master push and remote ancestry were verified.

## Scope and ownership

Owned checkpoint: `mcw/src/bitcoin_script.rs`, `mcw/tests/bitcoin_script_*`,
and this document. No Cargo manifest, second package, shipping executable,
platform binding or unsafe code is added. The only domain imports are `std::fmt`
and actual first-party `crate::bitcoin_encoding`. Source/vector notices, license
texts and hashes are retained in `bitcoin_script_fixtures/SOURCES.md` and manifest.

Preserved signature-hash checkpoint: `mcw/src/script_service/{mod.rs,sighash.rs}`,
its tests and fixtures. It uses actual first-party `bitcoin_wire` transaction,
outpoint and output types and `bitcoin_encoding::Sha256`; it adds no curve or
signature implementation. It was published immediately before the scope
correction arrived; no production registration or cutover followed.

Former broad caller ownership is inactive under the correction. Bounded candidates
being audited are `Extensions/NBitcoinExtensions.cs` classification/key-identifier
leaves and the Script text leaf in `Serialization/Bitcoin.cs`. The latter file
remains JSON-owner-owned. No existing managed file has been changed by this worker.
Script execution, BIP322, CoinJoin verification and whole transaction validation
are outside the revised assignment. A precise bounded assignment is required
before further caller work. QR retains
shared crate registration/manifests/host/dispatch/lifecycle/platform/packaging and
the shared ledger. Coordinator alone dispatches actual incorporation when QR is idle.

## Concrete format API

All fallible format functions return typed `bitcoin_script::Error`. Script data
and instructions are independent of IPC, C#, UI, OS handles and curve operations.

| API | Contract |
| --- | --- |
| Script::from_bytes/from_vec/from_hex | bounded raw byte container; accepts malformed syntax |
| as_bytes/into_bytes/to_hex | exact byte identity; lowercase hex |
| instructions/Script::instructions | borrowed, zero-allocation typed iterator; one error then fused |
| Instruction getters | opcode, byte offset, exact raw prefix/payload, PushEncoding, payload/minimality |
| validate/Script::validate | explicit push-minimality and push-size policy; stats/push-only classification |
| ScriptBuilder | bounded raw append, exact instruction append, minimal/length-only/explicit pushes, numbers |
| Opcode/opcodes | every wire byte, known names/aliases, opcode class, small integer, BIP342-success classification |
| encode/decode_script_number | signed magnitude LE; explicit 0..9-byte limit and minimality; i64 endpoints |
| is_minimal_script_number/cast_to_bool | number minimality and Script boolean/negative-zero rules |
| Script::from_asm | explicit Wallet/CoreFormat dialect; never guess numeric interpretation |
| to_wallet_asm | retained wallet hex-oriented display, typed error on malformed push |
| to_core_asm | Core RPC display without signature annotations; malformed suffix is [error] |
| to_format_asm | lossless Core FormatScript text, including malformed suffixes |
| witness_program | exact envelope v0..16, 2..40 bytes, including unsupported v0 lengths |
| classify_output/output_template | exact P2PKH/P2SH/P2WPKH/P2WSH/P2TR/P2A/P2PK/multisig/OP_RETURN/future envelopes |
| p2pkh/p2sh/p2wpkh/p2wsh/p2tr/p2anchor | construct from supplied fixed payloads only |
| witness_output | address-compatible v0 restrictions; v1..16 preserved explicitly |
| p2pk/multisig/op_return | retained formats; key shape only, no curve/signature/relay claim |
| script_from_address[_text] | actual bitcoin_encoding codec, explicit network, revalidate mutable fields |
| address_from_script | existing payload conversion, Option for non-address formats, invalid-v0 error |

Application bounds: 1,048,576 script bytes; 33,554,432 asm bytes; 9 number bytes;
20 multisig keys. These are resource bounds, separate from consensus/relay limits.
PUSHDATA4 declares a u32 length; truncated headers/payloads are checked before
slicing or copying and can never trigger allocation of the declared length.
Explicit builder additions check their complete size before mutation. Rust's
normal allocator behavior applies to accepted bounded allocations.

Raw/hex/CoreFormat round trips preserve unknown/reserved/disabled opcodes and all
nonminimal prefixes. Validation never edits data. `is_push_only` matches Core's
syntactic rule, including OP_RESERVED; this is not an execution-success rule.
Malformed streams return a typed error in validation/template classification;
raw construction, hex and CoreFormat still preserve them exactly.

Witness envelope recognition distinguishes invalid v0 lengths from P2WPKH/P2WSH,
P2TR, P2A and future/unknown v1..16. Address encoding is not evidence of a valid
point or spendability. Public-key recognition is only SEC1 length/prefix shape,
including historical hybrid encodings. P2PK and multisig address hashing is not
hidden in this module; direct address conversion returns None for those formats.
OP_RETURN construction has no relay-limit claim.

Wallet display asm intentionally retains hex semantics, `OP_CLTV`/`OP_CSV` names,
OP_UNKNOWN(0xNN) and the `OP_10..16` parser's historical hex interpretation.
Wallet display/parse can normalize nonminimal pushes and single-byte zero;
use hex/CoreFormat for identity. Core RPC display and Core test FormatScript
are separate representations. No signature/sighash annotation parser is claimed.
The template checker requires the exact P2PKH OP_EQUALVERIFY byte; it does not
reproduce NBitcoin 10.0.13's fast-check omission of byte 23.

## Proposed bridge range

Reserved range `0x0D00..0x0DFF`. This is an integration proposal, not implemented
host dispatch. Input script blobs must use the host's bounded, length-framed bytes.
Reject trailing bytes, unknown discriminants, invalid counts/encodings and invalid
UTF-8 before invoking domain code; preserve typed errors.

| Operation | Domain operation |
| --- | --- |
| 0x0D00 | parse instruction descriptors/validation stats, explicit policy |
| 0x0D01 | bounded typed serialization, explicit exact/minimal/length-only mode |
| 0x0D02 | render enum-selected Wallet/CoreDisplay/CoreFormat text |
| 0x0D03 | parse enum-selected Wallet/CoreFormat text |
| 0x0D04 | classify output and exact payload fields |
| 0x0D05 | construct typed output from existing payloads |
| 0x0D06 | Script number encode/decode with size/minimality policy |
| 0x0D07 | address/script conversion with explicit network |
| 0x0D20..0x0D3F | preserved signature-hash proposal; production registration pending bounded assignment |

Script CompactSize/witness wire framing belongs to actual `bitcoin_wire`; do not
implement a second transaction/witness serializer or normalize Script bytes there.

## Verified format evidence

Command, from `.artifacts/mcw-bitcoin-script`:
`./mcw/tests/bitcoin_script_verify.ps1` using shared existing Rust 1.99.0/edition 2024.
One of the two FileShare.None build slots was held; single-job compilation and
at least 2 GiB free memory enforced. No toolchain was copied or installed.

- Rustfmt and Clippy with `-D warnings`: domain, conformance and differential probe pass.
- Debug and optimized overflow-checked tests: 17 pass in each profile (16 Script
  tests plus one real encoding module length test).
- All 2,414 Script strings in Core v29.0's 1,207 noncomment rows: serialization,
  parser/error/minimality summaries, display and lossless round trips pass.
  Execution flags/results are excluded; this is format evidence only.
- 189 number creation vectors; explicit i64::MIN/MAX, overflow, negative zero,
  nonminimal/padded forms, arithmetic-4-byte and CLTV/CSV-5-byte boundaries pass.
- Eight retained literal application fixtures round trip, including the existing
  output-registration test's arbitrary transaction bytes stored as Script.
- Independent Python differential: 89,349 checks, including 68,292 number decodes,
  3,002 encodes, 6,000 parser/display/lossless-format cases and push boundaries.
- Adversarial 0xffffffff declared lengths, all unknown opcodes, malformed tails,
  exact prefix boundaries, atomic builder failures and application size bounds pass.
- Windows x64 native debug/optimized execution and metadata check pass. Linux
  x64/ARM64 and macOS x64/ARM64 target std is unavailable locally; no build/runtime
  result is claimed for those four targets.

Evidence root:
`C:/Users/user/OneDrive/Documents/ChatGPT/MagicalCryptoWallet/.artifacts/mcw-bitcoin-script/.artifacts/bitcoin-script-evidence/`.
`verification.json` SHA-256 `e130d20ddac277896eec270c041dcd02416da9c967b1a635211ebfa67e2d584e`;
`differential.json` SHA-256 `484f32fd11097615a29c0624050ad0fc9eef54d91e542c179d11dd470f2b8d7f`.
The ignored actual-source rustc test harness uses the installed linker's static CRT
for development tests only; it is never a shipping mcw artifact/runtime claim.

## Concrete signature-hash API and evidence

All fallible functions return typed `script_service::sighash::Error`. Digest bytes
are the raw SHA-256 order, without display reversal. The cache immutably borrows
the exact transaction and, when required, all spent outputs in input order.

| API | Contract |
| --- | --- |
| legacy_sighash | full u32 hash type, historical SINGLE result, parsed OP_CODESEPARATOR removal |
| without_code_separators | remove opcode instructions only; preserve separators inside pushed data |
| find_and_delete_signature | remove matching length-only serialized pushes at instruction boundaries |
| SighashCache::new/with_spent_outputs | bounded actual bitcoin_wire types; exact ordered prevout count |
| segwit_v0_sighash/SighashCache::segwit_v0 | BIP143, supplied amount and explicit sliced scriptCode |
| taproot_sighash[_message]/SighashCache::taproot[_message] | BIP341 epoch/message/tagged digest, explicit annex and TapScriptExtension |
| tapleaf_hash/tapbranch_hash/taptweak_hash | tagged hashes only; no point lift, group operation or commitment verification |

The caller retains responsibility for executed code-separator context, real
prevout binding, signature verification and execution/policy. Unsupported Taproot
hash types, missing SINGLE outputs, wrong prevout counts, annex/key-version errors
and bounds fail explicitly. The cache prevents stale mutation through Rust
borrowing; it does not verify supplied prevouts against a chain state.

Command: `./mcw/tests/bitcoin_script_sighash_verify.ps1`, using the same installed
toolchain and build-slot discipline as the format checkpoint. Rustfmt and strict
Clippy pass. Debug and optimized overflow-checked profiles each pass nine tests
(eight signature-hash tests and one real encoding module test). Exact published
cases pass: 500 Core v29 legacy hashes, ten BIP143 preimage/hash cases and seven
BIP341 epoch-message/hash cases. Independent Python differential checks pass
3,600 synthetic legacy, SegWit and Taproot cases, including annex/extensions,
full legacy hash types and output-mode boundaries. Fixtures exclude private keys
and signing data; source hashes/license notices are in `SIGHASH_SOURCES.md` and
`sighash_manifest.json`.

Canonical LF sighash source SHA-256:
`adc700286c4d45d81f89da7a3e4d75b0301081d907116358d95290d8e7952c30`.
Evidence under the same evidence root, `sighash/verification.json` SHA-256
`e71fef8bc206cd1edd0be0a65cf55456fda0e451cffd7f0db2c147d23407dcaa`;
`sighash/differential.json` SHA-256
`a66b95e507562957ce2a43dc95d6534f80620a5ffd065ab9792aad2659fa3a5a`.
These are Windows x64 development tests; other native targets and shipping/runtime
acceptance remain unverified. No validation or spendability claim follows.

## Remaining production/dependency acceptance

Inventory revision: 748a961c78980c42bba293ff7ad1b9ca696566ec.
Explicit matches: 535 Script-format references in 126 files; eight Script-validation
references in six files; six NBitcoin package references in five files; 56 lockfile
references in 14 files. Tests/comments are included; implicit imported references
require further audit. Nothing in the checkpoint removes these references.

Next permitted work is an audit of concrete minimal classification/serialization
caller replacements, followed by the coordinator's precise bounded assignment.
Full interpreter, signature verification, wallet-cryptography, CoinJoin and
transaction-engine expansion is stopped. Existing legacy paths are retained.
No fake verifier, managed fallback or stub is accepted.

Any bounded replacement requires its production paths to execute first-party Rust,
old NBitcoin calls eliminated from those paths, current caller/package/runtime
audits, shared integration and native five-target tests. Entire NBitcoin package
removal requires every remaining caller and packaged reference to disappear,
including transaction/key/PSBT/signature responsibilities owned by other workers.
The checkpoints alone do not establish any package removal.
