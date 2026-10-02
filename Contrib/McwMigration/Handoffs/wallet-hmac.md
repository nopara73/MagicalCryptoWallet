# Bounded wallet HMAC integration

This leaf replaces only three computations: OwnershipIdentifier HMAC-SHA256 and
Slip21Node seed/child HMAC-SHA512. Existing Key/Script/node ownership, encoding,
serialization, recovery and cryptography stay in their current owners. No whole
package is removed. The broad curve/SHA1/HKDF drafts are excluded.

## Verified handler checkpoint

`mcw/src/wallet_hash_service.rs` calls the exact published `wallet_hashes.rs`
checkpoint (canonical LF SHA256
`752aec6cb51d2603f9c447f12e4c043425bb31129c5b2a473bd63b78eeeeb4a2`).
It adds no hash implementation, dependency, unsafe code, native crypto library,
companion executable or secret CLI route. The handler borrows input and returns a
redacted response guard that clears its owned copy on Drop. Transport/caller
copies require their own clearing; this is best-effort, not a formal erasure or
constant-time guarantee.

| Operation | Request bytes | Response |
|---|---|---|
| `0x0A10` ownership identifier | key32 followed by original script bytes | full 32-byte HMAC-SHA256 |
| `0x0A11` SLIP21 seed | original seed bytes, including permitted empty seed | full 64-byte HMAC-SHA512 |
| `0x0A12` SLIP21 child | parent left32 followed by original label bytes | full 64-byte HMAC-SHA512 |

Seed domain key is exactly ASCII `Symmetric key seed`. Child hashing adds exactly
one `0x00` before the raw label. Binary NULs are preserved. No Unicode
normalization, script parsing, scalar validation or truncation is introduced.
String labels continue to use the retained caller's Encoding.ASCII semantics.
Requests use the existing bridge limit of 1,048,560 bytes excluding the header;
keyed operations require at least 32 bytes. Errors contain only static categories
and text. There is no logging or file/process activity in the handler.

Command: `./mcw/tests/wallet_hmac_verify.ps1`. Actual-source Rust 1.99.0/edition
2024 harnesses passed rustfmt, Clippy `-D warnings`, and 24 tests in both debug and
optimized builds with overflow checks. The suite retains the original 569 primary
hash vectors and adds 441 distinct independent Python stdlib/OpenSSL HMAC cases,
public SLIP19/SLIP21 paths, malformed key lengths, empty/binary labels, full output
lengths, exact frame capacity and one-byte overflow. Checksums and source/binary
hashes are recorded in ignored `.artifacts/wallet-hmac-evidence/verification.json`.
Test executables are tooling only and do not add a shipping package.

## Incorporation gates

QR owns `mcw/src/lib.rs`, `app.rs`, `bridge.rs`, shared platform/lifetime files and
`MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs`. The handler
checkpoint is not production caller replacement until the actual host registers
and dispatches these operations and its request/response copies are cleared on
success, cancellation, error, late response and disconnect paths.

Owned managed leaves are `MagicalCryptoWallet/Mcw/Crypto/WalletHmac.cs`,
`MagicalCryptoWallet/Crypto/OwnershipIdentifier.cs`, and
`MagicalCryptoWallet/Crypto/Slip21Node.cs`. They must execute the real bound host,
with no managed HMAC fallback or shadow computation. Caller publication follows
verified incorporation; synthetic actual-caller integration, cancellation,
failure/redaction and lifetime evidence is recorded separately from this primitive
checkpoint. Existing key/Script/domain state and supported MAC bytes stay intact.

SLIP39 PBKDF2/recovery is outside this assignment. KeyManager, curves, signatures,
mnemonics, encryption, full wallet cryptography and NBitcoin/Secp removal are also
outside scope. Target/runtime evidence is reported only for targets actually run.
