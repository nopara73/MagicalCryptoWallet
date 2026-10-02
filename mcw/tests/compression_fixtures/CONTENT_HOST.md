# Existing bounded content host proof

Use the current composed source checkout and its supplied shipping `mcw` binary.
The runner builds the actual Core project and links its actual
`ManagedApplicationHost.cs`, retained fee caller, cached factory and owned content
fixtures. Every referenced project's build output is isolated under fresh evidence.
Existing package lock files are honored. No native build or shared source patch
is performed.

```sh
python3 mcw/tests/compression_content_host_portable.py \
  --source-root /absolute/current/source \
  --native /absolute/shipping/mcw \
  --out /absolute/fresh/content-host-evidence
```

On Windows, use the existing Python interpreter and `mcw.exe`; the runner locates
the shared build-slot directory through Git and acquires FileShare.None via the
OS API. `--coordination-root` can select that existing directory explicitly.
Non-Windows CI jobs retain their owner's scheduling/resource guard. Builds use
one MSBuild worker. `--build-only` verifies compilation and preserves an explicitly
unverified native-run record.

The retained-path session checks **26 cases**: the exact named fee caller with
identity/gzip/zlib/Brotli/layered responses, corrupted/unsupported private failures,
response metadata, HTTP body acquisition cancellation, four reverse coding proofs,
15 malformed/bounded native packets, and a usable sibling after those errors.
Its SOCKS server is loopback-only and never resolves/connects to the requested
onion target. No wallet, UI, Tor daemon or public connection is used.

Separate raw child sessions exercise actual native cancellation, EOF and request
queue saturation. All REQUEST IDs increase in wire order; CANCEL names the
existing request. The child waits for the real handshake, completes the content
frame write, and injects the control/EOF/backlog. The only passing oracle is the
actual **22-byte Cancelled reply with input > 0 and 0 < output < complete size**.
This proves private decoding had started and had not completed. Cancellation also
requires a working later sibling; EOF must drain the interrupted reply before
host shutdown; saturation additionally requires the exact native queue-limit error
for the overflowing request. Success bodies and startup-only cancellations fail
that oracle.

There is no shipping wire acknowledgement for an inner decoder checkpoint. The
runner uses a bounded schedule search to obtain an observable partial-work reply;
spin counts or elapsed time are never acceptance evidence. If no trial satisfies
the counter/closure checks, that gate remains unsatisfied and the runner exits
with failure. This is distinct from the deterministic barrier/checkpoint proof in
`content_inbox_tests.rs`; `barrier_synchronized_native_checkpoint` stays false.
Do not call this runner a deterministic native-checkpoint barrier test.

Run the existing deterministic component proof against the same current source
checkout separately:

```sh
python3 mcw/tests/compression_content_inbox_portable.py \
  --source-root /absolute/current/source \
  --rustc /absolute/rustc \
  --out /absolute/fresh/content-inbox-evidence
```

This compiles exact copies of the actual Inbox, Frame and their dependency
closure without a shared hook or host patch. Seven cases in each debug/optimized
profile pause decoding at checkpoint 20, acknowledge reader ingress before
resuming, and require the private partial-work cancellation failure. Queued
REQUEST IDs exceed the active ID and increase in wire order. This is deterministic
component evidence; the supplied-binary runner remains the actual host proof.

`verification.json` binds current source and fixture hashes, exact supplied/staged
binary SHA256, logs, native exit codes, per-case partial-work counters and attempted
injection schedules. Intentional EOF/overload sessions require shipping native exit
1; orderly retained/cancel sessions require exit 0. Only test-owned processes are
started. Caller/build-owner evidence must establish that the supplied binary was
built from the pinned current sources; the runner pins both independently.

Windows x64 evidence from the composed `mcw-host-qr` sources records 26 actual
host cases and all three native interruption cases passing with binary SHA256
`9a62a0d48f14312bf047d403b507a11d0c6f41d59aedfdbf4397ae2328e02eec`.
That run is preserved in `portable-host-a/verification.json` under the owned
ignored evidence directory. A later composition/build requires a fresh paired
run; the earlier immutable binary copy and logs remain historical evidence.

Windows x64 local evidence does not certify Linux/macOS or the other architecture.
The host owner registers and executes this existing proof in the five-platform
gate. Managed packages, HttpClient/TLS, routing and the retained runtime remain.
