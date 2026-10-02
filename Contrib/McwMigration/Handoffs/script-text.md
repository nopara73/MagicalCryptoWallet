# Bounded client Script text replacement

State: **component verified; client/host incorporation pending**.
Worker: bitcoin-script, thread 01a0fc4b-dd8a-72b2-bed8-8298491abaa4.
This assignment replaces only the retained Script text parser and renderer in
the WabiSabi HTTP client. The previous format and signature-hash checkpoints
remain separate. Script interpretation, classification, signature verification,
transaction validation and NBitcoin package removal are outside this assignment.

## Implementation and explicit client role

`mcw/src/script_text.rs` uses `std`, the actual first-party `bitcoin_script`
module and its existing encoding dependency. It has no unsafe code, external
crate, IPC, OS/UI dependency or duplicate transaction/curve implementation.
`parse`/`parse_utf8` and `render` reproduce the retained NBitcoin 10.0.13 text
contract, including boundary Unicode trimming, interior ASCII vertical tab,
permissive `OP_UNKNOWN(0xNN` suffixes and the final `0` for truncated pushes.
The stricter format parser, instruction iterator and classifiers are unchanged.
Legacy display is lossy for nonminimal push encodings, as in the retained library.

`ScriptTextClient` requests the actual application service; its strict UTF-8 and
frame-size checks fail explicitly. It never launches a second service, falls
back to NBitcoin or shadows the native result. Managed NBitcoin `Script` remains
the raw data object, constructed from the returned bytes rather than text.

The prepared `ClientScriptTextJson` boundary explicitly selects WabiSabi client
request/response types. It has no environment, process-role or ambient detection.
The existing shared/coordinator serializers keep their managed entry points.

The exact existing caller leaves are:

- `WabiSabiHttpApiClient.InternalSendAsync`: output registration's `Script` field
  uses the native renderer; the seven script-free request types retain their schema.
- `WabiSabiHttpApiClient.GetStatusAsync`: a round-state response uses the native
  parser for `InputAdded.Coin.TxOut.ScriptPubKey` and
  `OutputAdded.Output.ScriptPubKey`, in construction/signing events.
- External `MagicalCryptoWallet.Coordinator/Startup.cs` keeps
  `Encode.CoordinatorMessage` and `Decode.CoordinatorMessageFromStreamAsync`.

The small prepared `Serialization/Bitcoin.cs` and `Coordination.cs` changes
parameterize only those Script leaves, reusing their existing object schemas.
Field order, defaults, aliases, credentials, witness/RPC/configuration codecs and
other transaction/cryptographic operations stay with their current owners.
The HTTP factory owner released these disjoint JSON encoding/decoding leaves.

## Single-host operation contract

| Operation | Request payload | Reply payload |
| --- | --- | --- |
| `0x0D08` | entire strict UTF-8 legacy Script text | exact raw Script bytes |
| `0x0D09` | entire raw Script bytes | strict UTF-8 legacy Script text |

The existing version-one frame has a 1 MiB total bound and a 16-byte header,
so either request/reply payload is at most 1,048,560 bytes. No extra count,
discriminant or trailing framing is added. Native error code 2 denotes a codec
failure; code 3 denotes an unsupported operation or a reply exceeding the frame
bound. The actual managed application service reports these errors as IOException.
The existing decoder's Catch behavior still produces a failed decode, with no
legacy parsing fallback. Declared PUSHDATA4 lengths cannot allocate their value.

QR owns `mcw/src/lib.rs`, `bridge.rs` and `app.rs` registration/dispatch. The
prepared narrow patch was applied only to an isolated verification checkout of
published `e64c080096a44614a1bfb4778782e8f3d3554c5b`. The coordinator dispatches
incorporation when QR is idle; the active QR checkout has not been edited.
The actual client caller cutover must be incorporated with host dispatch.

## Verified component evidence

`mcw/tests/script_text_verify.ps1` uses the already installed Rust 1.99.0, edition
2024, one shared build slot, a single job and at least 2 GiB free RAM.
Rustfmt and Clippy with warnings denied pass. Debug and optimized overflow-checked
profiles each pass eight tests (seven Script-text tests and the actual encoding
module's internal length test).

The development oracle is the application's exact retained NBitcoin 10.0.13
net10.0 assembly, SHA-256
`ebb7e5548fe1325514289528e67b2ee0e24b3bfecb75ed4067c44ea99a202167`.
It is not a shipping dependency or native fallback. Source/license provenance is
in `mcw/tests/script_text_fixtures/SOURCES.md` and `manifest.json`.

- All 10,914 fixed retained-library cases pass: 5,655 parses (980 expected errors)
  and 5,259 renders.
- All 8,914 fresh differential cases pass: 4,655 parses and 4,259 renders,
  using independent seed 218685131.
- Invalid/truncated push headers, declared u32 maximum lengths, unknown opcodes,
  lossy nonminimal pushes, whitespace/token quirks and explicit bounds are covered.

Canonical LF component source SHA-256:
`806262ff3747a3375dded0d0878a430f6cd0f1edb90f598fa6fb46cc7c9cb7ea`.
Fixed fixture SHA-256:
`5ae17e2109e8e430937b63721de354d63a7be1ce95adc9a74dbf34ef9a3d065f`.
Component evidence is under
`.artifacts/mcw-bitcoin-script/.artifacts/script-text-evidence/`.

## Production caller verification

The prepared complete patch builds the actual Core and Client projects cleanly:
zero warnings and errors. The development harness references those production
projects, rather than copies of the serializers, service or HTTP client.
All 26 client/coordinator routing checks pass. Distinct recording-service replies
prove that returned native text/bytes reach the actual JSON fields and Script
objects; these recording checks do not claim native algorithm execution.
In-memory HTTP checks exercise the real RegisterOutputAsync/GetStatusAsync methods.
Coordinator/stream serializers make zero native calls. Missing services and failed
native parsing cannot use the old client parser/renderer as a fallback.

All 18 checks through the actual mcw host/application connection pass, including
the retained text quirks, exact Script bytes, invalid request UTF-8, oversized
rendered replies and both production HTTP methods. Normal host shutdown exits 0.
The actual Windows x64 host was built with its established std/native-OS runtime;
the PE import audit passes, with no non-OS runtime imports. The application Cargo
metadata remains one package with zero external dependencies.
`mcw/tests/script_text_client_verify.ps1` rebuilds the actual single host using the
established Windows std/native-OS runtime, and runs a synthetic managed child via
the production ManagedApplicationHost connection. HTTP remains in-memory, with
no user wallet, data directory, coordinator, external network or visible window.
Its evidence will be recorded under
`.artifacts/mcw-script-text-verify/.artifacts/script-text-client-evidence/`.

This is bounded source and Windows x64 development evidence. Shared incorporation,
Linux/macOS targets, packaged artifacts, CI and release/runtime acceptance remain
separate. No whole-application, spendability or dependency-removal claim follows.
