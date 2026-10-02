# Client round fingerprint

**Publication state:** the owned leaf, unused adapter and verification assets
are published as a checkpoint. Production still uses its existing calculation.
The verified caller switch is in `coinjoin-caller-activation.patch`; its host
switch is in `coinjoin-host-registration.patch`. The coordinator must apply
both together during a fresh idle host integration batch. No old implementation
or shared dependency is retired by the unused checkpoint.

This handoff covers only the client-side round ID calculation used to reject
coordinator parameters that do not match their advertised ID. Rust owns that
pure calculation in `mcw/src/round_hash/`. The verified activation makes
`RoundState.IsRoundIdMatchingAsync` call it through the persistent host pipe;
the status updater awaits it before
accepting a new round. Credentials, proofs, transaction construction, signing,
round state and coordinator policy remain owned by their existing implementations.

The independent coordinator `RoundHasher.CalculateHash` and its STROBE code are
unchanged. WabiSabi, libwabisabi, NBitcoin and their other callers remain required.
This leaf does **not** remove those dependencies or migrate CoinJoin as a whole.
The withdrawn broad `coinjoin_service` drafts are not part of this deliverable.

## Operation and compatibility

Bridge operation `0x1200` has one typed payload and returns exactly 32 raw bytes.
It accepts no caller-defined transcript labels or hashing commands. The body
starts with `u16` version 1 and `u16` reserved zero. Integers and lengths are
little endian. Strings are `u32` UTF-8 length plus bytes. Script types are `u16`
count followed by those strings, in the original sorted set's enumeration order.

| Body order | Representation |
| --- | --- |
| Input registration start | `i64`, Unix milliseconds |
| Input registration, confirmation, output and signing timeouts | Four `i64`, .NET ticks |
| Input amount min/max; input script types | Two `i64`, satoshis; script sequence |
| Output amount min/max; output script types | Two `i64`, satoshis; script sequence |
| Network; mining fee | Exact `Network.ToString()` UTF-8; `i64` fee-per-k satoshis |
| Maximum transaction size | `i32` |
| Relay fee, maximum amount/vsize credential values, per-Alice vsize allocation, maximum suggested amount | Five `i64`, existing units |
| Coordination identifier | Exact UTF-8, including whitespace, BOM and replacement encoding of unpaired UTF-16 |
| Amount issuer Cw/I and vsize issuer Cw/I | Four fixed 33-byte compressed public points |

The transcript matches the retained reference byte for byte: `WabiSabi_v1.0`,
domain `round-parameters`, indexed script labels, field framing and 32-byte PRF.
The existing string overload appends `.Cw` even to `domain-separator`, network,
script names and coordination identifier. Both historical coordination-fee
fields are still hashed as signed 64-bit zero. Hash output is constructed as
`uint256(rawBytes)` without display-order hex reversal. Dates with different
offsets but the same instant and sub-millisecond changes retain their original
semantics. No sorting, rounding, normalization or policy validation is added.
Issuer points already have managed validated types; this hash leaf does not
replace curve validation or credential cryptography.

STROBE implements only meta-AD, AD and PRF, with explicit little-endian lane
conversion. It reuses the published first-party
`privacy_service::crypto::hash::keccak_f1600`; there is no second permutation,
external Cargo package, unsafe code or platform API in the domain module.

## Bounds, cancellation and failures

The payload cap is 1,048,560 bytes, leaving the bridge's 16-byte header within
its 1 MiB frame bound. Each string is at most 65,536 bytes and each script
sequence at most 64 entries. Native typed calls have the same total work bound.
Unknown version/reserved values, trailing bytes, invalid UTF-8, truncation,
oversized fields and wrong operations fail before hashing. The host returns a
generic typed error without echoing metadata. A malformed result length fails
in the managed adapter. There is no legacy hash fallback.

The existing poll cancellation/30-second deadline flows through the comparison.
The generic host adapter releases cancelled waiters and drains late replies;
this operation owns no mutable state or external side effects. Unavailable or
failed host calls use the updater's existing failure/backoff path and preserve
accepted state. Activation removes the old lazy hash cache so record copies cannot
retain a fingerprint for different parameter values.

State/awaiter unit tests explicitly inject the retained coordinator oracle
through the updater's optional async validator. After activation, application callers use the
default Rust validator. `RoundHashProbe` separately exercises that default
caller and updater through the real `mcw` and `ManagedApplicationHost`.

## Verification and integration

After applying both activation patches, `mcw/tests/round_hash_verify.ps1`
builds the probe with `RoundHashActivated=true`, audits the pinned unchanged managed source,
regenerates independent managed bytes in an ignored evidence directory,
checks the immutable fixtures, runs published STROBEgo checkpoints, malformed
wire/native bounds and continuation tests, then builds and audits the release
host and runs `Contrib/Mcw/test-round-hash.py`. Only synthetic public parameters
are used: no wallet, live coordinator, credentials or funds. The probe is a
verification executable and is never packaged.

The 67 STROBEgo cases cover initialization, AD/meta-AD/PRF, streaming and rate
boundaries. Test-only checkpoints after unsupported operations let those
published vectors remain independent without adding KEY/ENC/MAC to production.
The managed vectors cover mainnet/testnet/regtest, offsets, milliseconds/ticks,
signed integer limits, amounts/fees, script enumeration/custom comparers,
Unicode/whitespace/BOM/replacement encoding, 166-byte boundaries, maximum
identifier length and each issuer point. Actual-host checks cover both matching
and tampered IDs, concurrent requests, updater acceptance/failure, cancellation,
late replies, every truncation, invalid version/reserved/trailing fields and
recovery after malformed input. Payloads must not appear in stdout or stderr.

Shared host/ledger/CI integration is owned by the coordinator. The reviewable
registration patch enables this module and its sole dispatcher operation.
Caller activation must include that registration atomically so it never lands
against an unsupported host operation. The dependency ledger must classify
`RoundHashProbe` as verification, link this handoff for `round_hash`, and record
only this client fingerprint as migrated. Existing dependency removals are
unchanged. Native evidence is target-specific; Windows x64 evidence does not
establish Linux/ARM64/macOS runtime or release readiness. Global host CI repairs
and the other migration leaves remain their owners' work.

Sources: [STROBE 1.0.2 specification](https://strobe.sourceforge.io/specs/),
[STROBEgo vectors](https://raw.githubusercontent.com/mimoo/StrobeGo/master/strobe/test_vectors/test_vectors.json).
Exact oracle pins are in `mcw/tests/round_hash_vectors/provenance.json`.

The checkpoint also runs the exact unused leaf through its standalone
integration-test shim. Both probe guard modes compile on the pinned verified
checkout. A later managed build on master `88f916aa` was blocked by unrelated
`RpcObjectCodec` namespace/type errors; its owner was notified. That failure
and the precise validation baseline are recorded in `verification.json`.
Fresh host integration must check the then-current managed application build.
