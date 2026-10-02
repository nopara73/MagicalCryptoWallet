# Bounded wallet HMAC integration

This leaf replaces only three computations: OwnershipIdentifier HMAC-SHA256 and
Slip21Node seed/child HMAC-SHA512. Existing Key/Script/node ownership, encoding,
serialization, recovery and cryptography stay in their current owners. No whole
package is removed. The broad curve/SHA1/HKDF drafts are excluded.

## Verified handler checkpoint

Handler commit: `7ae424b5f5f3734ca1870962a2d913769c59b26d`. The module is now
exported on master. The typed leaf and review patches below await atomic host and
caller incorporation; the managed callers on master still use their original MACs.

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

## Verified actual caller candidate

`Contrib/Mcw/HmacProbe/verify.ps1` compiles the real core domain classes and actual
managed host source, then runs them as the child of an actual Rust host snapshot.
Its project and locked graph are `.inc` templates instantiated only under ignored
artifacts. It adds no application project, Cargo package, shipping executable or
new package version. `README.md` in that directory describes reproduction in an
isolated candidate with both review patches applied.

The final Windows x64 candidate is based on master
`a7d07f383ab75a7b7b7fbd9493000b46eb38ad0a`. The reviewed shared patch is applied
only to copied snapshot source; the active QR checkout and production shared
files were not edited. Locked managed restore/build passed with zero warnings and
errors; staged Rust host Clippy `-D warnings` and optimized compilation passed.
The run passed:

- 441 independent full-MAC fixtures through actual OwnershipIdentifier/Slip21Node
  callers, with 462 successful domain calls including public SLIP19/SLIP21 paths,
  retained inputs, six ASCII string-label cases and post-fault connection checks.
- Six adapter fault checks and three real-domain failure checks proving every
  replaced computation reaches the bound host instead of a retained managed MAC.
- 37 actual managed-transport fault/lifetime checks: successful ownership transfer,
  error-body redaction, invalid UTF-8, protocol rejection, canceled late responses
  and 32 delivery/cancellation races. Scripted replies are dummy fault buffers,
  never a hash implementation. Captured owned read/write buffers are cleared.
- Two actual Rust malformed-request rejections and one in-flight cancellation,
  followed by valid requests on the same usable connection. Stdout is empty and
  stderr contains none of the synthetic payload markers.

`wallet-hmac-evidence.json` binds the fixture, exact tested source bytes, canonical
Git LF source, test binaries, commands and patch hashes. Ignored evidence lives
under `.artifacts/wallet-hmac-evidence/caller-probe`. The native snapshot uses a
tooling-only static CRT and does not set the production Windows runtime cfg;
shipping-runtime removal, packaging and other four target executions remain
unverified. The proof does not claim formal erasure or constant-time execution.

| Review patch | SHA256 of Git LF bytes |
|---|---|
| `wallet-hmac-callers.patch` | `5294dca38c9353c2a1a07ed7efd8fddb28e213791981f0cdc99ba0fc071f6bd1` |
| `wallet-hmac-host.patch` | `c9b261176332c36e32957273c58b19b89297b1c51849e3ed4e38f296bac8e7c9` |

The two-caller patch changes only the three assigned computations and clears the
temporary ownership key copy. Key/Script/node state, full 32/64-byte outputs,
ASCII string conversion and exactly one child-prefix NUL are preserved. The
three-file shared patch dispatches the typed operations, clears transport-owned
copies on success/failure/cancel/disconnect paths, redacts native frame Debug and
managed service diagnostics, and converts invalid error UTF-8 to a static
IOException without embedding decoder bytes.

## Incorporation gates

QR owns `mcw/src/lib.rs`, `app.rs`, `bridge.rs`, shared platform/lifetime files and
`MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs`. The handler
checkpoint is not production caller replacement until the actual host registers
and dispatches these operations and its request/response copies are cleared on
success, cancellation, error, late response and disconnect paths.

QR/coordinator must apply the host and caller patches together when incorporation
is authorized and QR is freshly idle. `lib.rs` already exports the handler; the
shared patch no longer changes it. Regular wallet unit/integration suites must be
bound to the real host as part of that cutover. An ordinary standalone `dotnet
test` currently has no service binding. Do not replace it with a managed fallback
or silently omit those tests. Reconcile only the published hunks against later
shared updates and rerun the actual candidate and relevant retained suites.

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
