# Bounded JSON-RPC integration handoff

This handoff covers only application RPC request parsing, parameter/result
mapping, response serialization and HTTP body handling. Configuration, common
schemas, key files, caches, coordinator/provider serialization, wallet state and
domain methods are outside this change. Shared NBitcoin/Newtonsoft users remain.

The service and typed managed adapter are published unused. Production caller
activation is pending: apply both `../Patches/json-rpc-host.patch` and
`../Patches/json-rpc-callers.patch` in one verified integration after QR's fresh
idle state. Do not publish the caller switch without the host registration.
The host patch is proposed against foundation source
`7ae424b5f5f3734ca1870962a2d913769c59b26d`; reconcile its small dispatch hunks with
later shared registrations. The worker never edits QR's active checkout.

## Files and contract

- `mcw/src/serialization_service/` owns only RPC parse, one-response write,
  batch write and retained primitive parameter coercion. It uses the published
  first-party JSON engine and Rust stdlib, with no additional Cargo dependency,
  package, executable or runtime sidecar.
- `MagicalCryptoWallet/Mcw/Serialization/` is a typed binary adapter and explicit
  mappings for current application RPC values. It contains no JSON grammar,
  reflection JSON serializer, NBitcoin JSON converter or legacy parser fallback.
- The caller patch replaces Newtonsoft execution in exactly
  `JsonRpcRequest.cs`, `JsonRpcResponse.cs`, `JsonRpcRequestHandler.cs` and
  `JsonRpcServer.cs`. It retains method dispatch, optional/named arguments,
  initializer paths, cancellation-token injection and managed domain calls.
- `Contrib/Mcw/RpcProbe/` is developer/test-only. It checks that the caller patch
  is applied before running. It is absent from shipping project references,
  package manifests and application entry points.

Legacy constructor fields match ASCII case-insensitively, with the last
occurrence winning. Numeric and boolean IDs remain string IDs with their original
numeric spelling; omitted/null IDs remain notifications. Named parameter names
remain exact. Parameter object duplicates keep their last value and first key
position. Comments, trailing commas, single quotes and bare ASCII object keys
remain accepted in this RPC profile; engine defaults for other users are unchanged.

RPC money is **integer satoshis**, dates are **Unix seconds**, bytes are **lowercase
hex**, fee rates retain their decimal format, and outpoints use `TransactionId`
and `Index`. Integer tokens are never converted through floating point. Retained
float-to-integer coercion rounds ties to even; decimal number coercion follows
the old double reader's fifteen-digit behavior while decimal strings retain
their scale. Date-like parameter strings retain the old date detection/string
guard behavior. Standard caller error codes and captured messages are retained.

Two deliberate repairs are recorded separately in the fixtures: a batch drops
notification response slots instead of emitting stray commas; a null request
root produces a controlled parse error instead of throwing a null-reference
exception. Input is bounded UTF-8. Invalid UTF-8, oversized documents and other
non-JSON numeric extensions are rejected with the existing parse-error response.

## Bridge and resource ownership

Operations `0x0100..0x0105` are OPEN, APPEND, FINISH, READ, CLOSE and NUMERIC.
OPEN actions are 0=parse RPC, 1=write response, 2=write batch. Transfers have
monotonic nonzero identifiers, strict sequential offsets and explicit sizes.
NUMERIC modes are 0=integer, 1=decimal96, 2=boolean; results are a success byte
plus 16 little-endian bytes, or one failure byte.

Input is at most 8 MiB, typed/output transfer is at most 16 MiB, append chunks
are at most 512 KiB and reads at most 256 KiB. The native service permits eight
sessions and at most 64 MiB of retained transfer data; this is not an RSS claim.
Engine/tree limits also bound depth, nodes, entries, strings and number tokens.
The managed adapter permits four transfers, links caller/host cancellation and
uses a 30-second lifetime. Failed transforms drop inputs; final reads remove
results; cancellation closes the session; abandoned sessions expire after
30 seconds on the next operation. All sessions belong to one managed-child
lifetime and are dropped on child exit/restart. A missing/disconnected host has
no managed fallback.

## Verification and remaining integration gates

The legacy oracle was captured from actual baseline core RPC code before edits,
using synthetic services only. `legacy-golden.tsv` contains 83 original outputs;
`expected.tsv` records the two repairs and adds 12 existing RPC golden cases.
The runtime probe uses the actual `mcw daemon` host and production
`ManagedApplicationHost`, including chunked data, concurrent callers, cancellation,
rejected-transfer recovery, fail-closed disconnect and a loopback-only HTTP server.
No wallet, key, transaction, public listener or live funds are used.

`Contrib/Mcw/RpcProbe/verification-windows.json` records the final separate
candidate run: Rust formatting, Clippy with `-D warnings`, 13 native contract
checks, zero-warning/error managed core/client/probe/helper compilation, all
95 expected cases and the loopback/stress/disconnect checks passed. It includes
source, patch and actual native/managed binary SHA-256 hashes. The original
oracle provenance is in `legacy-provenance.json`. The helper compiles; the full
ordinary xUnit RPC invocation remains an integration gate. Windows evidence
does not establish runtime correctness on other targets.

The regular-suite binding change is included in the caller patch: its three
JSON-processing test methods launch the real host through `NativeRpcTest`, and
the test project references the developer probe with `ReferenceOutputAssembly=false`.
The helper is published unused. After atomic integration, build that probe with
its native host, run the regular RPC suite and preserve the existing domain-only
`BuildTransactionWithFees` test. Its new project reference must be reconciled
with concurrent test-project changes. Run the combined registrations and the
five target build/CI gates owned by QR; no such completion is claimed here.

`Contrib/Mcw/verify-json-rpc.py` reproduces the isolated native and managed
candidate, applies the exact proposed patches, runs Rust checks and the real-host
probe, and writes source/binary hashes. Hold one existing build-slot lock during
its actual build/test process, set `CARGO_BUILD_JOBS=1`, and release before waits
or publication. It never applies patches to the live checkout.
