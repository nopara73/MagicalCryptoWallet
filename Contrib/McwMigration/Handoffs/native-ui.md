# Bounded release-highlights Markdown replacement

Worker `native-ui`, chat `01a0fc5c-ff37-73e2-b69c-5d8a4834ef19`.
Scope: the retained release-highlights dialog only. Avalonia, ReactiveUI and
Skia remain. The preserved whole-UI drafts are excluded and unregistered.

State: corrected parser/typed adapter/headless checks, fresh proposed real-host
integration and current retained Fluent build verified. **No production cutover or package
retirement is claimed until the shared registration/caller/package batch lands.**

## Implementation and ownership

- `mcw/src/markdown/{mod.rs,inline.rs}`: first-party Rust/std-only bounded parser
  and typed presentation. No Cargo dependency, unsafe code, fetch, sidecar,
  native handle or general HTML/SVG/browser engine.
- `MagicalCryptoWallet/Mcw/NativeUi/MarkdownPresentation.cs`: strict schema
  adapter over the existing `IMcwApplicationServices.RequestAsync`; no managed
  Markdown parser, fallback or second connection.
- `MagicalCryptoWallet.Fluent/Controls/ReleaseHighlightsText.cs` and
  `Styles/ReleaseHighlights.axaml`: small existing Avalonia text/inline/link
  controls, native theme tokens, wrap/scroll and async cancellation/error state.
- Coordinator granted the exact retained `ReleaseHighlightsDialogView.axaml`,
  actual `ReleaseHighlightsDialogView.cs`, and isolated legacy
  `Styles/Markdown.axaml` leaves. Pending caller patch replaces the one old
  renderer and deletes its unused styles, retaining content/VM/dialog controls.
- QR owns shared host/crate exports, manifests/pins/lockfiles, native packaging
  and ledger publication. Reviewable patches: `native-ui-shared.patch` and
  `native-ui-caller.patch`. Incorporation is dispatched only when QR is idle.

The package has exactly one production renderer caller. Source dependency
closure of the actual Fluent lock identifies fifteen exclusive candidates:
Markdown.Avalonia, its Html/Svg/SyntaxHigh/Tight components, ColorDocument,
ColorTextBlock, AvaloniaEdit, Avalonia.Svg, ExCSS, Fizzler, HtmlAgilityPack,
ShimSkiaSharp, Svg.Custom, Svg.Model. This is a candidate graph calculation;
retirement must be confirmed from restored lockfiles and produced artifacts.

## Operation and behavior

Operation `0x1100`, schema `1`. Request: schema byte plus strict UTF-8 Markdown.
Response: schema byte/u32 block count, then each block's kind/level/depth,
length-prefixed marker/u32 run count, and each run's style/text/link/title.
Integers and lengths are little endian; strings are strict UTF-8. Tags are
paragraph0, heading1, list2, code3, quote4, rule5. Styles: bold1, italic2,
code4, strike8. Empty links/titles mean absent. The managed decoder rejects
unknown bits/tags, invalid levels/depth/UTF-8/NUL, unsafe links and trailing data.

Limits: 262144 input bytes, 1000000 output bytes, 4096 blocks, 32768 runs,
nesting32 and an eight-million-unit parser work budget. Tag/entity/autolink
windows are capped before scanning; each at-most64-byte chunk reserves work and
checks cancellation before inspection. Delimiter runs and reference closers use
the same accounting. Failed destinations and one-line reference parsing reserve
their linear suffix work before searching. Reference target URL/title clones and
emitted run metadata reserve work before comparisons/copies, including empty
reference labels. Diagnostics never echo input.
Headings, lists/lazy continuation, emphasis, links/reference links, line breaks,
historical literal `<br>` breaks, plain code, quotes and rules are supported.
Unsupported markup stays inert. This is the retained release-note grammar,
not a full CommonMark/HTML/asset engine. Only safe HTTP/HTTPS/mailto destinations
are link data; opening requires a click/keyboard action and follows existing
`UiContext.OpenBrowserAsync` confirmation.

Each view attachment owns its request. Replacement/detach cancels the wait and
generation checks suppress stale replies/errors. The real host's current
prepared registration passes `AtomicBool(false)`. **Native in-progress
cancellation remains an explicit integration gate** until the shared reader
passes its live request cancellation token to the parser and a runtime probe
proves it. Unit checks demonstrate token checks after charged search progress;
managed canceled waits/late-response draining are separate evidence. No parser fallback runs
if the host is absent. The old current packaged content extraction is preserved
exactly, including its empty `MarkdownText` result; no release copy is edited.

## Evidence and reproduction

`mcw/tests/native_ui_markdown_verify.ps1 -Format` acquires a shared build slot,
uses Rust1.99 single-job/native linker, runs nineteen grammar/bounds/fuzz/wire
tests, dispatches thirteen actual/historical inputs, decodes real Rust output
through the shipping managed leaf, and compares exact semantic blocks/levels/
depth/text/styles/link destinations with captured Markdown.Avalonia.Full11.0.3
results. All thirteen matched. The old XAML URI maps to the Full wrapper;
the immutable fixture records actual wrapper, versions, source hashes and
assembly hashes. No old parser package or code is checked into the replacement.
Historical feature names in test data do not reintroduce a feature.

The same verification uses the shipping renderer source and palette tokens for
sixteen light/dark, narrow/wide, 100/125/150/200-percent captures. It checks
keyboard link activation, malformed/truncated responses, asynchronous loading,
replacement/detach cancellation, stale completion and explicit bridge errors.
The dark/narrow and light/wide captures were inspected visually.

Strict Clippy on the shipping Markdown module and managed/headless builds
completed with zero warnings/errors. Current retained Fluent source also builds
with zero warnings/errors, with the new adapter/control present. Its live dialog
still uses the old renderer; the pending caller/style patch is not activated.

`native_ui_markdown_host_verify.ps1 -ProposedRegistration` builds an ignored
snapshot with the exact proposed shared registration and a synthetic test
child. It uses the shipping `ManagedApplicationHost.Connect`, common stream
and typed adapter with no real wallet, camera, Tor or network. The snapshot is
the same one mcw crate; its dev binary is not a second shipping executable.
After incorporation run the script without `-ProposedRegistration` to verify
the published host. Debug proof does not establish a native release import
audit or five-target shipping completion.

The fresh proposed real host exited cleanly after thirteen corpus inputs, four
malformed request rejections, two adversarial work-limit rejections, canceled
managed-wait recovery and twelve concurrent
requests over the one stream; the actual current packaged input yielded zero
blocks, preserving its existing extraction result.

After component publication, an ignored source snapshot of `6e8bf58cbf` with
both cutover patches restored all five consumers in Release configuration.
All fifteen Markdown-exclusive candidates disappeared and no package was added.
Tests/VisualPreview also omitted three Debug-only Avalonia packages under the
Release configuration; those remain conditional source references and are not
additional dependency retirements. Do not publish unrelated configuration churn.

That historical snapshot's caller artifact build was blocked by preexisting
`RpcObjectCodec.cs` compiler errors CS0234 at line8 and CS0118 at line14. The JSON
owner and coordinator received the exact errors; its source was preserved.
The shared repair is now present at base `2a091f3dd7`, and the current retained
Fluent source compiles. Neither result establishes proposed-cutover artifact
retirement; actual shared host/caller/package integration remains pending.

The previous saved host snapshot's `inline.rs` differed from the initially
published compiler bytes through Clippy refactors. Its old proof records,
snapshot source, binary and thirteen inputs are preserved under
`.artifacts/native-ui-markdown-history/audit-before-20261002T140413Z/`, with a
preservation/hash manifest. They are historical evidence, not a retrospectively
bound publication proof.

New runs use fresh directories and `native_ui_markdown_bindings.py`. It checks
the immutable fixture hash and thirteen exact input bytes, records raw/LF source
hashes before/after compilation, rejects changes during a run, and requires the
snapshot Markdown source to match the actual working parser bytes exactly.
Publication binding compares captured source Git blobs with a concrete commit.
The source hash manifest is evidence for that commit, not whichever later
master happens to contain it. Snapshot registration remains proposed.

Latest final candidate evidence at base `b5e3777b8e` (with the metadata-budget
follow-up):

- `.artifacts/native-ui-markdown-verification/runs/20261002T142815970Z/`:
  nineteen tests, strict lint, thirteen exact legacy comparisons and sixteen
  renderer cases; 1283 captured compiler source inputs unchanged during the run.
- `.artifacts/native-ui-markdown-verification/host-runs/20261002T143015054Z/`:
  matching parser bytes, actual shared managed host, thirteen exact corpus
  comparisons, malformed/adversarial/concurrent checks, clean exit, current
  retained Fluent build, and before/after source and built-binary hashes.

The earlier seventeen-test candidate at `2a091f3dd7` remains preserved. Full
publication binding rejected its reuse after an unrelated content adapter
changed on rebase; it is not the final publication proof. Bind the final runs
to their concrete published commit, rerunning on a pinned published checkout
if the source comparison reports any concurrent compiler-input change.

The immutable fixture SHA256 remains
`02e8b976e6671d70f07eca2c354b0014c2486065c8f9ef9d3b01ffb72328002e`.
Both final run manifests bind all 1283 captured source Git blobs to published
`b40d06ea2e397e9282a24f1f06f6e68f8077d3d4`; the before/after hashes match,
and the snapshot parser raw bytes equal the actual compiler inputs.
`native-ui-evidence.json` persists the concrete commit, source-set/parser/input
hashes, checks, binary hashes, historical-record locations and pending gates.
Activation, operation registration, native in-progress cancellation and
fifteen-package retirement flags remain false until atomic integration/proof.

`native_ui_markdown_retirement.py --report <path>` checks the actual caller,
host export/dispatch, absent legacy styles/pins/references, all five restored
locks and compiled Fluent dependency manifests/assemblies. It rejects today's
unchanged production state. `--root <snapshot> --proposed` keeps unpublished
snapshot evidence explicit; `--artifact-directory` can point to the retained
managed release output. A passing report does not prove native-host execution,
native imports or remote publication. Run the real-host proof separately.

Evidence is under the worker's ignored
`.artifacts/native-ui-markdown-verification/`; screenshots are not staged.
All verifier locks release in `finally` before source review/publication.

## Incorporation sequence

1. Apply the disjoint component commit, shared registration and pending caller
   patches in the same isolated integration checkout. With `core.autocrlf=true`,
   use `git apply --check --ignore-space-change` and then apply with the same
   option; both patches passed that applicability check at `6e8bf58cbf`.
   Remove the old package
   reference and central pin; regenerate all five affected production/consumer
   locks (Fluent, Desktop, Tests, BridgeProbe, VisualPreview) without peer churn.
2. Run bounded verification, actual published-host/caller build, dependency
   source/lock/artifact checks, and applicable native release/CI checks.
3. Publish the batch normally to master under the shared publication lock,
   verify remote ancestry and report the exact commit. Update the worker handoff
   with actual production integration/package retirement evidence.

No whole UI/help/wallet rewrite, general platform edit, deployment readiness
claim or replacement assignment belongs to this bounded work.
