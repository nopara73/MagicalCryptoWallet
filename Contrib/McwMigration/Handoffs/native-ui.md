# Bounded release-highlights Markdown replacement

Worker `native-ui`, chat `01a0fc5c-ff37-73e2-b69c-5d8a4834ef19`.
Scope: the retained release-highlights dialog only. Avalonia, ReactiveUI and
Skia remain. The preserved whole-UI drafts are excluded and unregistered.

State: parser/typed adapter/headless checks, proposed real-host integration and
production caller build verified. **No production cutover or package
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
nesting32 and a finite parser work budget. Diagnostics never echo input.
Headings, lists/lazy continuation, emphasis, links/reference links, line breaks,
historical literal `<br>` breaks, plain code, quotes and rules are supported.
Unsupported markup stays inert. This is the retained release-note grammar,
not a full CommonMark/HTML/asset engine. Only safe HTTP/HTTPS/mailto destinations
are link data; opening requires a click/keyboard action and follows existing
`UiContext.OpenBrowserAsync` confirmation.

Each view attachment owns its request. Replacement/detach cancels the wait and
generation checks suppress stale replies/errors. The real host's current
bounded synchronous dispatch may finish before it reads CANCEL; no claim is
made that host CPU parsing is interrupted in flight. No parser fallback runs
if the host is absent. The old current packaged content extraction is preserved
exactly, including its empty `MarkdownText` result; no release copy is edited.

## Evidence and reproduction

`mcw/tests/native_ui_markdown_verify.ps1 -Format` acquires a shared build slot,
uses Rust1.99 single-job/native linker, runs twelve grammar/bounds/fuzz/wire
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
completed with zero warnings/errors. The production Fluent caller, including
its compiled bindings and link codebehind, also built with zero warnings/errors.

`native_ui_markdown_host_verify.ps1 -ProposedRegistration` builds an ignored
snapshot with the exact proposed shared registration and a synthetic test
child. It uses the shipping `ManagedApplicationHost.Connect`, common stream
and typed adapter with no real wallet, camera, Tor or network. The snapshot is
the same one mcw crate; its dev binary is not a second shipping executable.
After incorporation run the script without `-ProposedRegistration` to verify
the published host. Debug proof does not establish a native release import
audit or five-target shipping completion.

The proposed real host exited cleanly after thirteen corpus inputs, four
malformed request rejections, in-flight wait cancellation and twelve concurrent
requests over the one stream; the actual current packaged input yielded zero
blocks, preserving its existing extraction result.

Evidence is under the worker's ignored
`.artifacts/native-ui-markdown-verification/`; screenshots are not staged.
All verifier locks release in `finally` before source review/publication.

## Incorporation sequence

1. Apply the disjoint component commit, shared registration and pending caller
   patches in the same isolated integration checkout. Remove the old package
   reference and central pin; regenerate all five affected production/consumer
   locks (Fluent, Desktop, Tests, BridgeProbe, VisualPreview) without peer churn.
2. Run bounded verification, actual published-host/caller build, dependency
   source/lock/artifact checks, and applicable native release/CI checks.
3. Publish the batch normally to master under the shared publication lock,
   verify remote ancestry and report the exact commit. Update the worker handoff
   with actual production integration/package retirement evidence.

No whole UI/help/wallet rewrite, general platform edit, deployment readiness
claim or replacement assignment belongs to this bounded work.
