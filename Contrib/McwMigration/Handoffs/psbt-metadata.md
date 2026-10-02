# Bounded PSBT metadata caller replacement

This prepared assignment replaces the live wallet's `AddKeyPaths`, `AddKeyPath`, and
`AddPrevTxs` helpers. The two calls in `TransactionFactory.BuildTransaction` become
one typed `McwPsbtMetadata.Enrich` call. Construction, selection, fees, signing,
finalization, script/policy checks, wallet state, and result types retain their
existing owners. The stopped broad transaction drafts are untouched, unregistered,
and excluded from publication. No removed import/export UI is restored.

## Ownership and shared integration

Owned leaves are `mcw/src/psbt_metadata.rs`, `psbt_metadata_service.rs`,
`mcw/tests/psbt_metadata_*`, `MagicalCryptoWallet/Mcw/Psbt/`, this handoff and its
activation patches. Existing caller edits are restricted to the three helpers and the two
factory call sites. The QR integrator owns `mcw/src/lib.rs`, `app.rs`, bridge,
Cargo, CLI, platform, packaging, and the migration ledger. The prepared
`psbt-metadata-host.patch` is for idle shared-owner incorporation.
`psbt-metadata-caller.patch` contains the factory change and helper deletion;
`psbt-metadata-activation.patch` combines both. Host routing, caller activation,
and the integrator's real-host managed-test fixture must be incorporated
atomically. The patches do not change the active host checkout or the 1 MiB
bridge frame ceiling.

Component publication `a7d07f383ab75a7b7b7fbd9493000b46eb38ad0a` activated the
caller too early. Narrow repair `1bdd57f1639c8e77e9f346c85f08e420b0584fee`
restored only the two call sites and three helper bodies. Current runnable
`master` retains those original helpers; the new adapter is unused until the
complete activation is integrated. This temporary retention is not a runtime
fallback in the replacement adapter.

The managed adapter reads native input outpoints/WITNESS_UTXO scripts and output
scripts, asks the existing KeyManager and transaction store for their own data,
and sends public key/fingerprint/path/script associations and available parent
bytes. Rust distributes origins, writes PSBT records, verifies supplied parent
hashes, output indexes and matching witness UTXOs, and preserves other records.
The resulting packet is loaded into NBitcoin solely for the retained typed
signer/result boundary; packet network and every PSBT setting are preserved.
The replacement adapter has no old-helper fallback or validation shadow.

## Compatibility

Input wallet lookup uses WITNESS_UTXO alone, then output lookup follows, matching
the removed helper. Origin distribution happens before parent attachment. A
newly available parent cannot retroactively create an input origin. Existing
NON_WITNESS_UTXO takes precedence when resolving a coin, including the old
null-coin behavior for an incorrect hash or output index. Finalized inputs are
skipped; an all-finalized packet skips all origin edits, including outputs.

Origins are the original BIP174 compressed-public-key records even for an
explicitly supplied Taproot script. No BIP371 origin or key tweak is invented.
Known keys also match legacy P2PKH, compressed P2PK and multisig recipients,
coherent P2SH/P2WSH metadata and the former public-key redeem-script inference.
Uncompressed P2PK and multisig keys remain distinct from the wallet's compressed
keys. Smart P2SH-P2WPKH redeem insertion follows `Settings.IsSmart` and leaves an
existing redeem script intact. Native candidate indexes avoid a quadratic scan
of every key and every map.

The factory uses standard network builder extensions. Arbitrary custom managed
builder plugins are rejected by this bounded leaf. Missing parents remain
best-effort omissions. Supplied parents with incorrect hashes, invalid referenced
indexes or conflicting witness values/scripts fail before committing a packet.
This is metadata consistency, not signature, consensus or spendability proof.
The retained signer and script/policy checks remain authoritative.

Unknown/proprietary records, all untouched key/value bytes, unsigned transaction
bytes and native map order are preserved. Replacement retains its original
position; new records append. NBitcoin's typed load/export normalizes map order
as before; compatibility compares both complete records and canonical managed
bytes, rather than promising that adding records retains the old serialized
packet byte-for-byte.

## Typed operations and bounded transfer

All integers are little-endian. A blob is length:u32 followed by bytes. Every
payload/reply begins with version:u16 = 1; no native handles or bridge frames
enter the domain layer, and no packet bytes are logged.

| Operation | Payload after version | Reply after version |
| --- | --- | --- |
| `0x0600` enrich | PSBT blob, smart byte, origin count and public-key33/fingerprint4/path-count/indexes/script-blob entries, parent count and txid32/transaction-blob entries | enriched PSBT blob |
| `0x0601` inspect | PSBT blob | input count and txid32/vout-u32/witness-presence/[script-blob] entries, output count and script blobs |
| `0x0602` begin | target operation:u16, total bytes:u32 | session:u64 |
| `0x0603` append | session:u64, exact offset:u32, chunk blob | session:u64, next offset:u32 |
| `0x0604` commit | session:u64 | session:u64, result bytes:u32 |
| `0x0605` read | session:u64, exact offset:u32, requested bytes:u32 | session:u64, offset:u32, chunk blob |
| `0x0606` abort | session:u64 | session:u64 |

The production managed adapter uses sessions for both typed operations. Chunks
are at most 64 KiB; every frame remains below the original 1 MiB ceiling. Total
request/result bytes are limited to 32 MiB, PSBT bytes to 16 MiB, values to 4 MiB,
and packet records/maps to the established PSBT limits. Embedded transaction
decoding retains the published wire codec's limits. At most four provisional
sessions and 64 MiB of declared session buffers exist per managed connection.
The managed leaf serializes its transfers with a cancellable semaphore; wallet
state queries occur outside that semaphore.

Offsets and counts are checked before allocating or appending. A malformed
append/commit/read for a recognized session deletes its provisional upload/result. Only a complete upload
can commit; only a complete, ordered result can reach the retained signer.
Final read automatically drops the native result. Abort is idempotent. The
host's request-ID cancellation drops the associated provisional session, EOF
and protocol failure clear all sessions, and restart/connection destruction
drops their owner. Managed error paths abort their known session, and the
existing reader drains late replies. Buffer clearing is best effort, not a
secure-memory erasure claim.

## Verification and completion gates

The independent fixture generator executes NBitcoin 10.0.13's original
metadata operations on synthetic packets. Its cached assembly SHA256 is
`ebb7e5548fe1325514289528e67b2ee0e24b3bfecb75ed4067c44ea99a202167`.
Thirteen small fixtures are checked in; the fourteenth uses a complete parent
above 1 MiB and remains an ignored generated artifact. Coverage includes legacy
recipients, nested/witness scripts, compressed/uncompressed keys, Taproot,
smart-off, missing fingerprint/parent, parent-only/original bad parent,
finalized/part-finalized inputs, origin replacement, v2 and witnessed parents.

```powershell
./mcw/tests/psbt_metadata_reference.ps1
./mcw/tests/psbt_metadata_verify.ps1
./mcw/tests/psbt_metadata_verify.ps1 -Optimized
./mcw/tests/psbt_metadata_host_patch.ps1
./mcw/tests/psbt_metadata_host_verify.ps1 -BuildOnly
./mcw/tests/psbt_metadata_host_verify.ps1 -NativeApplication <fresh-integrated-mcw.exe>
# Before shared activation, select a tracked-source snapshot with the complete patch:
./mcw/tests/psbt_metadata_host_verify.ps1 -CoreSourceRoot <activation-snapshot> -NativeApplication <snapshot-mcw.exe>
```

Verifiers hold a shared build slot only during actual compilation/testing,
report slot/PID, use one build job/codegen unit, require 2 GiB free RAM and release
the slot in `finally`. Native checks snapshot actual first-party sources and
deny Clippy/compiler warnings. Transfer failures exercise offset/replay,
truncation, cancellation, session/aggregate limits and private cleanup.

The real-host probe compiles the actual modified managed core and actual
ManagedApplicationHost with a synthetic child. It compares old/new packet bytes,
fees, settings, unsigned bytes and complete parents, then exercises the actual
factory's unsigned and signed paths and the retained signer/policy validator.
The child uses the host's supported GUI executable name
`MagicalCryptoWallet.Fluent.Desktop` to avoid colliding with the core assembly
on Windows; normal host disposal sends the empty shutdown payload.
The verifier refuses execution if its selected factory source still contains
the old call sites. `-BuildOnly` may validate the inactive core and records that
state explicitly. An isolated activation snapshot proves the candidate only;
the exact published integration must pass again before completion.
It uses no real wallet or broadcaster. Until the shared routing patch is
incorporated and that exact native application passes this probe, production
host integration remains pending. Existing managed transaction tests also need
the integrator's shared real-host fixture; there is no test-only legacy fallback.

NBitcoin remains a required managed package for builders, signer, keys, scripts,
typed PSBT results and other live callers. This leaf adds no external Rust
dependency or shipping executable and makes no package-removal or platform
release claim. Further verification/publication IDs and evidence are recorded
only after successful checks and remote verification.

Current native debug and optimized evidence:
`.artifacts/mcw-psbt/.artifacts/psbt-metadata-validation/20261002-204142-575-3b142ecf/evidence.json`
and `.artifacts/mcw-psbt/.artifacts/psbt-metadata-validation/20261002-204902-416-4af5cbe5/evidence.json`.
All twelve tests passed in both modes, including the fourteen independent metadata comparisons
and the complete request/result transfer above 1 MiB. Formatting and Clippy with
warnings denied passed. This is component evidence; actual-host validation is
still pending. The actual modified managed core and synthetic host child also
build with zero warnings/errors; build evidence is
`.artifacts/mcw-psbt/.artifacts/psbt-metadata-host/20261002-205531-076-3c836481/evidence.json`.

The combined activation snapshot based on published repair `1bdd57f` now passes
the actual GUI host/managed IPC probe: two old/new packet comparisons (one with
a complete parent above 1 MiB), unsigned and signed factory builds, unchanged
fees/settings/unsigned bytes, retained signing/policy validation, and clean
shutdown. The native application compiles with warnings denied, and the managed
core/child build has zero warnings/errors. Candidate source/archive/patch hashes
are in `.artifacts/mcw-psbt/.artifacts/psbt-evidence/metadata-candidate-source-1bdd57f.json`;
probe evidence is
`.artifacts/mcw-psbt/.artifacts/psbt-metadata-host/20261002-211644-273-e6b953da/evidence.json`.
Only tracked source plus the complete activation patch was used; the existing
managed WabiSabi build-output DLL was copied from the shared checkout and its
hash recorded. No source-graph stubs or removed daemon mode were used.
This is candidate integration evidence. Published shared routing, the managed
suite fixture, and verification of that exact published application remain
pending; the currently published caller still uses its original helpers.
