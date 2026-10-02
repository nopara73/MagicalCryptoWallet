# Wallet hashes: bounded checkpoint

The hash implementation is verified and ready for module incorporation. No
production caller or upstream package has been replaced by this checkpoint.
The human's 2026-10-02 scope correction stopped the full wallet cryptography,
key and recovery migration. Work is limited to independently replaceable hash
responsibilities and concrete caller integration after ownership is assigned.

Implementation commit: `6343f04fc8da3cb80773683905fab1411d87023e`

## Ownership and scope

Owned source is `mcw/src/wallet_hashes.rs`, corresponding `mcw/tests/wallet_hashes_*`
files and this handoff. First-party SHA256 is the actual `bitcoin_encoding` sibling
published in `6365f3244d23b801c0bb36581967caad8aae968e`. No SHA256 implementation is
copied. No Cargo manifest/lib/host/CLI/bridge/platform/packaging file is edited, no
additional package or shipping executable is created, and no wallet/key data is
read or changed. Original code is repository MIT; vector provenance and the Trezor
vector MIT license are preserved in `wallet_hashes_fixtures/SOURCES.md` and manifest.

The unpublished curve, SHA1 and HKDF drafts and their test evidence are preserved
in `.artifacts/mcw-wallet-hashes`. They are unregistered and outside this bounded
checkpoint. The latest curve compatibility run failed one deterministic ECDSA
edge case; it must not be counted as a production capability. No complete key,
signing, mnemonic, recovery, encryption or wallet-state rewrite is authorized by
this checkpoint. The clean `.artifacts/mcw-wallet-hash-callers` checkout separates
any bounded follow-up from those preserved drafts.

## Portable API

- `Ripemd160`, `Sha512`, `Hash160`: `new`, checked `update(&[u8])`, consuming
  `finalize`; cloneable prefixes, redacted Debug. One-shot `ripemd160`, `sha512`,
  `hash160` return byte arrays through `Result`. Digests use specified byte order.
- `HmacSha256`, `HmacSha512`: `new(key)`, checked `update`, consuming `finalize`
  through `Result`, and `verify(&[u8])`. Empty/short/block-sized/long binary keys
  follow HMAC. Full 32/64-byte MACs are required by verification. One-shot HMAC and
  verification functions are available. Explicit truncation belongs to callers.
- `constant_time_eq`: equal-length inputs are visited completely with optimization
  barriers; mismatched public lengths reject immediately. No Rust/compiler/CPU
  constant-time guarantee is asserted.
- `pbkdf2_hmac_sha256`, `pbkdf2_hmac_sha512` allocate a checked output; `_into`
  variants use caller-owned output without allocation. Parameter errors leave it
  unchanged. Password/salt are bytes; no Unicode normalization is implied.
- All diagnostics are static variants/messages and contain no supplied bytes,
  lengths, salts, keys, or digests. The source forbids unsafe and has no IPC, C#,
  UI, OS handle, crypto DLL, process or file dependency.

Proposed bridge allocation (integrator controls the actual secret policy):

| Operation | Proposed ID |
|---|---|
| RIPEMD160 / SHA512 / HASH160 | `0x0A00` / `0x0A01` / `0x0A02` |
| HMAC-SHA256 / HMAC-SHA512 | `0x0A10` / `0x0A11` |
| Verify full HMAC-SHA256 / HMAC-SHA512 | `0x0A12` / `0x0A13` |
| PBKDF2-HMAC-SHA256 / PBKDF2-HMAC-SHA512 | `0x0A20` / `0x0A21` |

No bridge dispatcher is implemented here. Avoid exposing arbitrary secret-bearing
operations, CLI arguments, logging, diagnostic payloads or persistent debug buffers.

## Admission and secret handling

RIPEMD160 checks `u64::MAX / 8` bytes, SHA512 `u128::MAX / 8`, and reused SHA256
checks its published limit. Excessive updates are rejected before mutation. HMAC
accounts for its 64/128-byte inner prefix through the actual streaming hash state.
PBKDF2 rejects zero iteration/output, limits each password/salt to 1 MiB, output to
65,536 bytes, iterations to 1,000,000, and total hash-compression work to 16,000,000
blocks. Work includes password normalization, one shared salt prefix, all PRF
iterations and output blocks; the four-byte big-endian counter cannot wrap.
These are admission limits, not password-hardening recommendations. Existing SLIP39
uses `2500 << iterationExponent`; exponent 9 and above exceed the iteration limit
and require explicit audited recovery/job-policy handling rather than silent change.

Compression schedules/branches use public round indices, never secret-indexed
lookups. MAC checks accumulate every equal-length byte. Buffers/state owned by the
new hashes are cleared in safe Rust with optimization barriers where practical.
This is best-effort clearing: compiler copies/registers/caller buffers and the
reused SHA256 private state are not guaranteed erased. Cloning duplicates keyed
state. Hardware/compiler timing, lifetime management and independent cryptographic
review remain production gates; this checkpoint makes no constant-time, secure
zeroization, FIPS validation or production-security claim.

## Exact evidence

Evidence directory:
`C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-wallet-hashes\.artifacts\wallet-hashes-evidence`

Command: `./mcw/tests/wallet_hashes_verify.ps1`. It acquires one exclusive shared
build-slot handle, requires 2 GiB free memory, uses Rust 1.99.0/edition 2024 and
single-job actual-source ignored rustc harnesses; no stub and no extra Cargo package.
Results: rustfmt and library/tests Clippy `-D warnings` passed; 18 Rust tests passed
in debug and optimized with overflow checks. One test belongs to the actual
SHA256 sibling; 17 cover this module. 569 offline primary vectors include all
257 NIST SHA512 ShortMsg/LongMsg cases, 14 RFC4231 HMAC cases, both RFC7914
PBKDF2-SHA256 cases and 288 multilingual BIP39 PBKDF2-SHA512 cases. Published
million-a, public SLIP21 and retained SLIP19 ownership identifier cases also pass.

`wallet_hashes_reference.py` uses Python 3.14.7 stdlib hashlib/hmac (independent
OpenSSL 3.5.7 development reference, never shipped). 7,228 distinct differential
cases passed in each debug/optimized profile: 1,340 each RIPEMD160/SHA512/HASH160,
1,222 each HMAC variant and 382 each PBKDF2 variant. Cases cover all 0..259 message
lengths, padding/block splits, long/empty keys, binary NULs, multiline output blocks,
1 MiB input limits, 65,536-byte output and 1,000,000 iterations. Refer to
`verification.json`, `differential-results.json`, `debug-tests.txt`,
`optimized-tests.txt` and `target-checks.json`; these bind exact source/binary hashes.
After adding the retained ownership vector, 18-test conformance was rerun; the
hash-module and differential-source fingerprints are unchanged from that run.

| Source | SHA256 |
|---|---|
| wallet_hashes.rs (LF/source bytes) | `752aec6cb51d2603f9c447f12e4c043425bb31129c5b2a473bd63b78eeeeb4a2` |
| conformance.rs (LF/source bytes) | `dc9efad9fbea3dfd6904d7498c666faee840e91e5dfcdbc15ba687ec12951f5a` |
| vectors.tsv (LF/source bytes) | `a62b8e37c19b94041b8e4c2f09a93cd632b09d7142ae7967b815c02060e8a7e9` |
| SHA256 sibling (actual CRLF checkout bytes) | `55f2002f3e2e91a8e0567523a5b1c8df778d95ffef71604ed87241af71ab13c2` |
| SHA256 sibling (canonical LF Git blob) | `4e59e9210317f6c75f1196d22892202b3c72f62adcf9d05be88c4c3c66925ec0` |

Windows x64 actual harness execution and metadata compilation pass. The other four
target standard libraries are not installed; Linux x64/ARM64 and macOS x64/ARM64
build/link/runtime/native-API acceptance remain unverified. Ignored test harnesses
use the installed linker/static CRT for execution; these are tooling, never mcw
shipping executables or shipping-runtime-removal evidence.

## Retained caller/package mapping and acceptance work

`wallet_hashes_callers.json` records audited master revision
`875e929193d11106766603767a939d1985a431cf`, source hashes, eight retained caller
files, nine direct hash call sites and six NBitcoin/Secp package references.
Retained callers include:

- OwnershipIdentifier HMAC-SHA256 over key/script bytes; existing SequenceEqual
  comparison requires deliberate full-MAC verification migration.
- Slip21Node HMAC-SHA512 over seed and 0x00-prefixed label bytes. Managed Key and
  secret ownership still exist.
- SLIP39 share HMAC-SHA256 (explicit four-byte truncation) and Feistel
  PBKDF2-HMAC-SHA256 with step/passphrase/extension-aware salt bytes. Remaining
  interpolation, mnemonic/wordlists, checked exponent and recovery services remain
  unchanged and outside the bounded hash replacement. Migrating the Feistel call
  would require preserving all supported exponents; the published PBKDF2 admission
  limit must not silently narrow recovery compatibility.
- Tor SAFECOOKIE System.HMACSHA256 remains managed; transport/handshake/logging
  ownership remains separate.
- ProofBody SHA256 can use the existing first-party sibling, but serialization and
  signing callers remain managed.
- KeyManager uses NBitcoin Mnemonic/ExtKey and other key/curve/script behavior;
  primitive BIP39 vectors do not provide Unicode or mnemonic behavior.

NBitcoin 10.0.13, NBitcoin.Secp256k1 3.1.6, managed platform crypto callers and
transitive managed packages remain transitional. No package is removed. The Rust
hash module has zero external Cargo/runtime/library/companion dependencies.

The smallest candidate integrations are the HMAC-SHA256 computation in
`OwnershipIdentifier.cs` and the seed/child HMAC-SHA512 computations in
`Slip21Node.cs`. They preserve existing Key/Script/data ownership and need no curve
implementation. These are proposed leaves, not a completed production cutover.
SLIP39 share HMAC is another possible leaf; Feistel PBKDF2 remains separate because
of the exponent compatibility issue above.

Remaining bounded acceptance work: agree precise caller ownership; register the
actual handler and typed adapter through QR's host incorporation; verify
secret-bearing request/response lifetimes, disconnect/error behavior and exact
SLIP19/SLIP21 caller outputs with synthetic inputs; retire only the replaced hash
calls; and record actual target/runtime evidence. QR's host/service boundary is
currently unpublished, so an adapter or primitive harness alone does not prove
production caller replacement. The broader key/recovery engine stays retained.
NBitcoin and Secp remain dependencies until every relevant caller and packaged
reference disappears. QR/coordinator control incorporation dispatch.
