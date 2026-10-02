# Tor control codec handoff

Worker: `01a0fc5c-d23b-7400-8819-68134acaf062`.
Reservation: `0x0F00-0x0FFF`. The wallet uses incremental operations
`0x0F02` (begin), `0x0F03` (feed), `0x0F04` (close). Single-shot reply/line
operations `0x0F00` / `0x0F01` retain their existing packet contracts.

The authorized responsibility is the wallet's **Tor control reply and CRLF-line
parser**, under `mcw/src/privacy_service/control_codec/`, with its typed adapter
under `MagicalCryptoWallet/Mcw/Privacy/`. Full Tor/backend expansion remains
stopped by the human correction. Existing protocol drafts are preserved.

The current source correction is
`f5af3f9d616faac27b3250308cc980155bed921a`: cooperative interruption in the
native domain and **18 native tests**. Its verification does not include a fresh
runtime host composition. The earlier incremental codec/adapter handoff
`441c981e6c63f5eede2d50885b96cbe292ccd910` has a separate historical isolated
GUI proof, described below. The existing QR integrator owns current shared
host/CI composition and fresh runtime verification. Production integration,
retirement of the current parser and dependency removal remain unverified here.

## Exact integration

Apply these together, reconciling concurrent hunks without replacing whole files:

- `privacy-control-callers.patch` replaces the wallet's reply/CRLF readers with
  typed Rust-service calls. It threads an explicit reader delegate through
  `TorControlClient`, its factory and `TorProcessManager`. Constructors are public
  and source compatible; the optional delegate defaults to Rust. There is no
  service-availability check or managed fallback. This patch is byte-identical
  to the prior `ee4319b1a3ff6e456eda916273f50a12ae2ffdf6` handoff.
- That patch retains the original managed reply parser exclusively in the
  external coordinator project and explicitly supplies it at construction.
  **Compose the SOCKS owner's coordinator-only readiness override** when applying
  the combined cutover. Privacy does not change `IsTorRunningAsync`; its public
  constructor, readonly delegate and `InitTorControlAsync` contract are preserved.
  Coordinator readiness must remain usable without a wallet host.
- The historical `privacy-control-host.patch` registers the small `tor_control`
  module alias, routes all five operations through `service::dispatch`, and adds
  a `ChildScope` guard for each managed-child lifetime. The guard removes abandoned
  native reads on every return/restart, including broken pipes. For the current
  interruption correction, the QR integrator must compose `dispatch_control`
  and the host cleanup contract below with its shared cancellation machinery.
  The historical patch alone does not activate that API. Its two proposed CI
  steps remain unexecuted in the historical proof.

Wallet automation removal `18f0619221` is preserved. The probe is GUI-only;
no retired managed executable or host mode is restored.

The historical GUI proof used base
`76b5c878cc27f6ae3182f2a77730fb15c163b1c2` with the exact runtime hunks and
`441c981e6c63f5eede2d50885b96cbe292ccd910` source additions applied. Its runtime
hunks were applied and executed; the workflow hunk was checked for applicability
but was not applied to that checkout or executed in CI. This is not a runtime
proof for `f5af3f9`. Do not mix the incremental adapter with the former
two-operation dispatcher. Preserve other workers' registrations, framing,
cancellation, lifecycle handling and readiness composition hunks.

## Current interruption API

Under the proposed host alias `tor_control`, the domain entry points are:

```rust
service::dispatch_control(
    operation: u16, payload: &[u8], interrupted: &AtomicBool,
) -> Result<Vec<u8>, service::ServiceError>

service::Readers::dispatch_control(
    &mut self, operation: u16, payload: &[u8], interrupted: &AtomicBool,
) -> Result<Vec<u8>, service::ServiceError>

stream::Decoder::feed_control(
    &mut self, bytes: &[u8], eof: bool, interrupted: &AtomicBool,
) -> Result<Scan<Reply>, Error>
```

`AtomicBool` is `std::sync::atomic::AtomicBool`. The host supplies a flag that
changes monotonically from false to true for one request; setting it on CANCEL,
EOF or overload is the host's responsibility. The domain uses Acquire loads at
entry, before/after begin insertion, before projection/encoding allocations,
at line boundaries and completion, and every 256 bytes of scanning, ASCII
projection and response copying. These cooperative checks do not preempt an
allocator call or guarantee a wall-clock cancellation deadline.

`ServiceError::Interrupted` is an out-of-band service result for the host to map
to its existing interrupted-request behavior. `Decoder::feed_control` returns
`Error::Interrupted`, clears its retained buffers and becomes terminal; discard
that decoder instead of resuming it. An interrupted begin removes its newly
inserted session; an interrupted feed removes its reader, including interruption
while encoding a NeedMore acknowledgement. A valid `CLOSE` always removes the
reader and acknowledges cleanup even when its flag is true; it is idempotent.
The original `dispatch`/`feed` APIs, normal packets and Tor parser rejection
categories remain unchanged. Interruption adds no Tor wire error category.

The global `service::dispatch_control` takes a single blocking `Mutex<Readers>`
for BEGIN/FEED/CLOSE and holds it through decoding and response encoding. Waiting
for that mutex is not interruptible, and `ChildScope` cleanup takes the same
mutex. The instance `Readers::dispatch_control` requires `&mut self`; locking
outside that instance is its owner's responsibility. Native checkpoint tests
do not prove shared-queue fairness or cancellation/EOF timing under contention.

A flag can change after the final domain check and return. If the host then
suppresses a successful BEGIN or NeedMore acknowledgement, it must explicitly
close that reader with `service::dispatch(service::CLOSE, &payload[..8])`, or
clear it through the child-lifetime guard on exit. That close uses the validated
request's known reader ID and does not depend on the interrupted flag. The
domain's last check cannot atomically couple its return to a transport write;
the QR integrator owns this post-return cleanup and its runtime proof.

## Compatibility and resources

Rust owns a retained decoder for each active read. The adapter sends **only new
chunks**, up to 16 KiB each, then releases their pipe bytes; completed replies
report consumption relative to the current chunk. Coalesced subsequent replies
remain in the pipe. No managed Tor grammar or raw-prefix staging remains.

The compatibility projection preserves terminal `250 OK` and data-block `.`
lines, skips blank continuation lines, keeps backslashes/stuffed dots literal,
maps high bytes to ASCII `?`, and retains historical .NET first-status handling
including unknown enum values, whitespace/signs and trailing NULs. EOF error
classifications/messages, caller cancellation, bridge shutdown, typed errors,
no-service rejection and malformed-response rejection are covered. Exact CRLF
scanning fixes the former bare-CR position bug, including split delimiters.

Admission is 512 KiB **total consumed input per read**, 64 KiB per line, 16,384
projected lines and 16 simultaneous native reads. These are local application
limits, not Tor-specified limits. A client-chosen positive ID is known before
begin; `finally` attempts close with an independent one-second cleanup token,
including canceled/late begin. Delivery under overload is not guaranteed.
Completion and parse rejection also remove the session. Close is idempotent.
The ordered session map requires no randomized hashing or extra runtime import.

Native counters verify each input byte is examined once and each completed line
is projected once. Tracked decoder buffer capacities are bounded by
`MAX_INPUT + 2 * MAX_LINE + MAX_LINES * size_of::<String>()` (1 MiB on x64).
This excludes allocator metadata, the bounded session map, response encoding and
process memory. The maximum encoded response is 589,840 bytes, inside the existing
1 MiB bridge frame. Begin/feed/close packets are bounded and typed; no payload is
placed in arguments or added diagnostic logs. Work/capacity bounds do not promise
a CPU deadline or real-time scheduling.

## Current native verification

For source `f5af3f9d616faac27b3250308cc980155bed921a`, the retained verification
record reports **18 native tests**, zero failed/ignored, with strict Clippy
(`-Dwarnings -Dclippy::all`) and rustfmt passing. This adds six tests to the
previous 12: exact default/controlled packet compatibility; cancellation after
begin insertion; active-feed scan, projection and encoding cancellation; and
pre-canceled cleanup/global API/quota recovery. Test-only checkpoints synchronize
a separate thread setting the real AtomicBool during those operations; this is
not an inference from a timed cancellation after a feed finished. No observer
callback is compiled into production.

The six source/test file hashes and API identity are recorded in the ignored
`.artifacts/mcw-privacy-control-reconcile-20261002/.artifacts/privacy-control-verification/control-interruption-evidence.json`;
the 18-test result is in `rust-tests.txt` in that same directory. The coordinator's
read-only audit confirmed these exact hashes and result. These checks establish
native domain behavior only. They do not establish current host composition,
post-return transport cleanup, EOF/overload handling or saturated shared queues.

## Historical incremental GUI verification

Source `441c981e6c63f5eede2d50885b96cbe292ccd910` was verified on native Windows
x64, Rust 1.99.0/edition 2024 and .NET 10. All measurements in this section belong
to that historical source and isolated composition:

- **12 Rust tests**, none ignored: 24 independent original-reader fixtures,
  8,000 independent .NET status-prefix cases, every fragment/EOF boundary,
  coalescing, exact limits, CRLF/bare-CR, invalid requests, 2,048 deterministic
  hostile lengths, one-byte feeds across a full 512 KiB reply, bounded work/buffers,
  quota/terminal cleanup and child-lifetime cleanup. Strict Clippy and formatting
  pass. This historical codec count is 12; the current native correction has 18.
  The previous eight-test codec and broader checkpoint counts are distinct.
- The full production core/client/probe builds with **zero warnings/errors**.
  The actual native shipping-runtime Rust host passes **102 GUI assertions**,
  with the real production readers and bridge.
  These cover normal producer backpressure, exact replies/errors, async events,
  synchronous responses, cancellation and negative transport cases.
- A source-bound GUI resource profile completes a 512 KiB reply with 2,048
  tail fragments: 2,049 pipe reads, 7,707,328 managed allocated bytes
  and 413 ms completion. Concurrent QR requests remain responsive (maximum
  10 ms). It cancels **33 near-cap reads** after Rust has consumed their
  prefixes, with maximum cancellation 6 ms, then reserves all 16 native
  reader slots successfully. These are measurements from this run; the guards
  are 64 MiB allocations, 20 s completion, 3 s cancellation and 1 s QR response.
- The pinned original `ee4319` adapter fails the same fragmentation/allocation
  guard after 43 reads: 68,673,000 allocated bytes in 168 ms, with maximum QR
  response 4 ms. This demonstrates repeated allocation, not observed host
  starvation. The incremental decoder removes prefix replay/rescanning.
- The Windows release build and first-party import audit pass with only Windows
  system libraries and no VC++ Redistributable import. Audited host SHA256:
  `89405817fa182b9e471c9340890762adc23a1435b67f9c76cec3925d05ea297c`.
  Cargo remains one package with zero external normal/build/test crates.
- The retained coordinator/Tor tests previously passed 34 checks with exit 0.
  They were not rerun for the incremental or interruption corrections; their
  exact caller patch is unchanged.
  The SOCKS owner separately verifies the combined coordinator readiness role.

Reproduce current native codec checks with `mcw/tests/privacy_control_verify.ps1`
or Cargo's `privacy_control` integration test. The following commands reproduce
the historical GUI profiles only when run with their matching source/runtime
composition; fresh host proof for `f5af3f9` belongs to the QR integrator:

```text
python mcw/tests/privacy_control_host.py --binary <mcw-path>
python mcw/tests/privacy_control_host.py --resources --binary <mcw-path>
```

Hold one exclusive shared build slot, use one build job and require 2 GiB free
memory. The probe uses synthetic data and an ephemeral loopback TCP server; it
does not ship. Exact historical normalized source hashes, runtime patch hashes
and measurements are in `privacy-control-evidence.json`. Ignored detailed
logs are under `.artifacts/mcw-privacy-control-gui-verify-20261002/.artifacts/privacy-control-verification`.
That evidence file is preserved unchanged and does not cover the later
interruption correction.

Still unverified: activation on remote `master`, packaged real-Tor smoke,
incorporated exact-commit CI, Linux x64/ARM64 and macOS x64/ARM64 execution,
saturated shared-queue cancellation/EOF and late-begin cleanup under overload.
Runtime patch execution was verified for the historical `441c981e` composition
only; its workflow hunk is proposed only. No current runtime claim is added.

## Retained dependencies and earlier checkpoint

Tor, the managed daemon/controller/transport, OpenSSL, Libevent, zlib and the
bundled Linux C++ runtime remain. The external coordinator retains its managed
parser. No shared package/native dependency is marked removed. Tor state, guards,
isolation, SOCKS policy and readiness keep their existing owners.

The broad protocol-only checkpoint `5d7d2c06de698ff3f5122b3c9e916c220f934135`
and scope correction `875e929193d11106766603767a939d1985a431cf` remain preserved.
Their 14 tests/inventory are not a Tor backend or release and do not authorize
resuming Tor/circuit/consensus/TLS/curve/storage/UI/CoinJoin rewrites. The small
`privacy_service::crypto::hash::keccak_f1600(&mut [u64;25])` API is unchanged
for the separately authorized round-hash worker; no second permutation is added.
