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

## Probe provenance qualification

The original 8,914-case report is historical. Its compiled probe SHA-256 is
`79194dba0f675da058fb5a95479d045b9175f7ba13ebc11c3a811a661ec3dee2`;
it does not certify the discovery-repair probe. The report is preserved unchanged
as `historical-pre-discovery/differential.json`, SHA-256
`b87cd9b1c821c431bbcb46ed05f56fd427c817a503174562a997f48b0372e458`.
The corresponding published pre-repair probe source hash is
`c0f091b63a98020931471672d7f83956d0b65284d87cc9da0ee1822e8f8611d1`.
The old executable was replaced at its named path; its retention and original
generated wrapper are not verified.

Discovery repair `d1b797733309674266930ab53460caca9c459bdc` changes test discovery
imports, with current probe source SHA-256
`aa0b499e28f4710db714854c088bf10641fbd867d62637143c737b4f17f3817d`.
The existing compiled Windows development probe SHA-256 is
`e3eb306c3c5c44dac4d8927ecd6568cf0873f598686f54155a631f91a1952481`;
its generated wrapper hash is
`dd540b09d39f8528f50daa3b194e8d979998ece0ad08856036a3ba3699eea7fb`.
The component implementation hash above is unchanged.

One comparison replay binds all 8,914 cases to that exact current probe, using
the same independent seed and cached retained-library oracle. No domain rebuild
or fixture regeneration was performed. `differential-e3eb306c.json` has SHA-256
`40e4a1e2e85c02f5f39bae6e6076a163337ab93ecaf8071b6da5f21767fedfe7`.
`probe-provenance.json` records both reports, all eight physical source hashes,
their canonical LF hashes and the oracle hash. The source set matches the pinned
published discovery-repair tree after line-ending normalization;
`bitcoin_encoding.rs` uses CRLF in the development checkout.

These results certify the named development probes and pinned sources. They do
not certify current master, an incorporated dispatcher/client, five native
targets or shipping artifacts. Actual incorporation still requires source
reconciliation and evidence tied to its resulting build; historical comparisons
must not silently become current-build evidence.

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

The refreshed caller evidence is a frozen candidate based on component commit
`dfdf6ba4c641ab49e7ce74d02d4119f98eb164d0`, with patch SHA-256
`3b914906a829487c67b8172d2efabe527985b1cb89b55fae4ab4ccf535a084e6`.
It is not current-master execution evidence. Shared host operations and client
caller cutover remain inactive; the coordinator controls QR-idle incorporation.

This is bounded source and Windows x64 development evidence. Shared incorporation,
Linux/macOS targets, packaged artifacts, CI and release/runtime acceptance remain
separate. No whole-application, spendability or dependency-removal claim follows.
