# PNG export handoff

State: **ready for integration**. The portable encoder and independent verification
are complete. Application-host declarations, bridge dispatch, managed/native save
adapters and release packaging remain with the QR/application-host task. This is
component evidence, not a claim that the application has migrated its export path
or removed Skia/Avalonia.

- Worker: `png`, thread `01a0fc2a-3c9a-7e81-b6e5-d198194b3f77`.
- Implementation and tests commit: `10f0805acdc0db07a70560a7fdfa1fcc7f2ae49d`.
- Publication: normal push to `origin/master`; the machine handoff records the
  final publication commit containing this document as well as the implementation
  commit. The document cannot contain its own Git commit hash.
- Inspected starting remote commit: `82127991068522210cdcf77080dc9b819502e486`.
- Reserved bridge proposal: `0x0300` through `0x03FF`.

## File ownership

Only these paths belong to this track:

| Path | Purpose |
| --- | --- |
| `mcw/src/png.rs` | Portable production encoder, including PNG, CRC32, zlib, Adler32 and DEFLATE |
| `mcw/tests/png_contract.rs` | Standalone std-only test harness and synthetic oracle fixture generation |
| `mcw/tests/png_verify.py` | Non-shipping independent Pillow/zlib conformance oracle |
| `Contrib/McwMigration/Handoffs/png.md` | This integration contract and evidence |

No Cargo package, manifest, module declaration, host, command, bridge, managed
adapter, QR matrix/encoder, receive UI, release pipeline or migration ledger was
created or edited by this track. All generated executables and fixtures are under
the worker checkout's ignored `.artifacts/png-verification/` directory.

## Portable API and behavior

```rust
let matrix = png::QrMatrix::new(width, height, row_major_modules)?;
let layout = png::Layout::AtLeast { width: 512, height: 512 };
let dimensions = png::dimensions(matrix, layout)?;
let bytes = png::encode(matrix, layout, png::BitDepth::Monochrome1)?;
```

`QrMatrix` borrows exactly `width * height` bytes. Index `y * width + x` is
coordinate `(x, y)`, with the first row at the top. Value 1 means black, value 0
means white; all other values are errors. Input excludes the quiet zone. The
encoder adds exactly four white modules on every side, uses one positive integral
scale in both axes, and emits only opaque black/white pixels. It accepts arbitrary
rectangular binary matrices, without claiming to validate QR symbol structure.

`Layout::Scale(s)` uses exactly `s` pixels per module. `FitWithin { width,
height }` chooses the largest scale fitting both requested dimensions and omits
unused outer canvas space. `AtLeast { width, height }` chooses the smallest scale
meeting both minimum dimensions. Every output has dimensions
`(module_width + 8) * scale` by `(module_height + 8) * scale`.

`Layout::default()` preserves the existing **512-pixel export minimum**; its
dimensions snap upward to whole modules. For a 21-module square this is 522 by 522
at scale 18; for 177 modules it is 555 by 555 at scale 3. An exact 512-pixel square
cannot simultaneously have whole modules and exactly four modules of quiet zone
when the padded symbol width does not divide 512. This snapping is deliberate.
An initial scope notification mentioned 300 pixels before inspecting the caller;
512 is the verified current constant and the implemented contract.

`BitDepth::Monochrome1` is PNG grayscale color type 0, one bit per pixel, with
white=1 and black=0 and leftmost pixels packed in the high bits. Unused low bits
are zero. `Grayscale8` uses white=255 and black=0, with no intermediate shades.
Both modes are non-interlaced and use filter None. Files contain precisely the
signature, IHDR, one IDAT, and IEND, with no alpha or ancillary metadata.

Bounds: 1..1024 modules per input side, 1..16384 output pixels per side, and at
most 67,108,864 output pixels. Explicit errors cover zero dimensions, excessive
matrix size, exact-length mismatch, nonbinary modules, zero scale, invalid/too
small canvas, excessive output dimensions/pixel count, checked arithmetic
overflow, and failed allocation. Only one scanline and the exactly reserved PNG
output are allocated. No full decoded pixel grid or separate compressed buffer
is created. No matrix, path, wallet data or environment is logged by the encoder.

The first-party DEFLATE writer uses RFC 1951 **stored blocks** of at most 65,535
bytes. It emits valid byte alignment, little-endian LEN/NLEN and exactly one
terminal BFINAL flag, including exact block boundaries. Stored blocks deliberately
do not reduce the scanline byte count; the default packed one-bit format keeps
typical QR exports small without a general-purpose compressor. Zlib framing uses
`78 01`, no preset dictionary, and Adler32 over every uncompressed scanline byte,
including filter bytes. PNG chunk integers/checksums are in network byte order;
CRC32 covers chunk type and data. This is a complete PNG encoder, without an
external zlib/image implementation hidden underneath it.

## Adapter and bridge proposal

The host can declare `pub mod png;` and call the API within the single `mcw`
executable. The module has no IPC or platform types. Existing Rust QR `Symbol`
data is already row-major: pass `symbol.modules` with its square width directly,
without copying, padding, rotation or transposition.

For the transitional managed adapter, preserve the current draw orientation:
`QrCode.DrawQrCodeImage` reads `source[i, j]` and draws at `(x=i, y=j)`. When
exporting its unpadded `bool[,] Matrix`, flatten `source[x, y]` into
`modules[y * width + x]`. Use `Matrix`, rather than the already padded
`FinalMatrix`; retaining the current two-module padding before this encoder would
produce six modules of margin. The Rust-symbol-to-managed adapter must similarly
assign `matrix[x, y] = symbol.modules[y * width + x] != 0`. Production orientation
must be checked with an asymmetric matrix and a real synthetic QR payload after
the adapter is connected.

Suggested operations within the reserved range:

| Operation | Proposed purpose |
| --- | --- |
| `0x0300` | Validate matrix/layout and return raw PNG bytes |
| `0x0301` | Validate the same input and return resolved width, height and scale |

One possible request payload, if it fits the host's current envelope, is
`width:u32le, height:u32le, layout:u8, depth:u8, reserved:u16le=0,
arg_width:u32le, arg_height:u32le, modules:[u8; width*height]`.
Layout 0 is Scale (`arg_width=scale`, `arg_height=0`), 1 is FitWithin and 2 is
AtLeast. Depth is 1 or 8. Reject unknown modes/depths, nonzero reserved fields,
wrong payload lengths and trailing bytes. `0x0301` can return three u32le values;
`0x0300` returns PNG bytes without a second encoding. These are proposals, not
implemented/shared bridge operations. The integrator owns the final wire contract
and error mapping.

For export, retain the existing save dialog, address-based filename, `.png`
extension behavior and cancellation behavior. Ask the host for PNG bytes and use
the established platform/managed file-writing adapter. Remove the export-only
`RenderTargetBitmap`, drawing context and `rtb.Save` calls after integration.
Derive a finite, positive pixel minimum from the existing display/export size,
bounded by the encoder limits and at least 512. Select AtLeast to meet that size
with whole modules. Keep file dialogs, filesystem writes, platform handles and
unsafe bindings outside this domain module.

The host's request/response frame limits must be reconciled with the encoder
bounds. A maximum-size grayscale result is slightly larger than 64 MiB. Apply a
documented smaller export bound if the bridge cannot carry that result, or raise
the host limit deliberately. Never truncate a PNG or silently bypass a failed
export by falling back to a third-party image implementation.

## Verification and independent evidence

Verified on Windows x64 with `rustc 1.99.0 (b940084d7 2026-09-28)`, edition 2024:

- 14 std-only Rust tests passed; one fixture-emission test is intentionally ignored
  during the ordinary suite and passed separately through the Python runner.
- All 270 synthetic PNGs independently inflated with the installed zlib oracle and
  decoded with Pillow 12.1.1 (`zlib 1.3.1.zlib-ng`), totaling 24,961,731 PNG bytes.
- Every pixel, four-module margins, integral scale, top-left row-major orientation,
  opacity, both bit depths and final-byte packing matched an independent expected
  raster. Coverage includes all 40 standard QR matrix sizes, asymmetric rectangular
  matrices, all-black/all-white inputs and every packed byte remainder.
- PNG chunk order/lengths/CRC32, zlib header/FCHECK/Adler32, complete independent
  inflation, and stored-block LEN/NLEN/BFINAL were checked. Scanline streams of
  exactly 65,535, 65,536 and 131,070 bytes cover terminal and cross-row block cases;
  additional Rust vectors cover zero and neighboring stream lengths.
- Standalone Clippy checks for production module and tests passed with
  `-D warnings -W clippy::all`; `git diff --check` passed.
- Static-CRT test executable imports only native Windows DLLs:
  `api-ms-win-core-synch-l1-2-0.dll`, `bcryptprimitives.dll`, `KERNEL32.dll`,
  `ntdll.dll`, `USERENV.dll`. No VC redistributable, Skia, zlib or image DLL appears.
  This audit describes the non-shipping harness, not a packaged application.

Reproduction, from the worker checkout (set the shared provisioned tool paths and
use an existing developer shell's native linker libraries):

```powershell
$env:CARGO_BUILD_JOBS = '1'
$env:RUSTUP_HOME = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-tools\rustup'
$env:CARGO_HOME = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet\.artifacts\mcw-tools\cargo'
& "$env:CARGO_HOME\bin\rustc.exe" --edition=2024 --test -D warnings -C target-feature=+crt-static mcw\tests\png_contract.rs -o .artifacts\png-verification\png-contract.exe
& .artifacts\png-verification\png-contract.exe --test-threads=1
python mcw\tests\png_verify.py --encoder-tests .artifacts\png-verification\png-contract.exe --output-dir .artifacts\png-verification\fixtures
& "$env:CARGO_HOME\bin\clippy-driver.exe" --edition=2024 --crate-type=lib --emit=metadata -D warnings -W clippy::all mcw\src\png.rs --out-dir .artifacts\png-verification\lints
& "$env:CARGO_HOME\bin\clippy-driver.exe" --edition=2024 --test --emit=metadata -D warnings -W clippy::all mcw\tests\png_contract.rs --out-dir .artifacts\png-verification\lints
```

On this machine the static libraries required by the test linker are in
`C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\MSVC\14.51.36231\lib\onecore\x64`
and
`C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\ucrt\x64`;
adding those directories to this shell's `LIB` was sufficient. Nothing was
installed or changed globally. Heavy verification held one of the coordinator's
two exclusive build-slot handles and ran only with more than 2 GiB free memory.
Reproduction must follow the same resource limits and publication-lock policy.

Ignored evidence under the original worker checkout
`.artifacts/mcw-png/.artifacts/png-verification/`:

- `fixtures/verification.json`: per-PNG hashes and complete oracle results.
  SHA256 `8d9de651f5efbde45ed843a920a9bffbdd6bbebfb81e2bb8cb95e78f1fdeec26`.
- `dependency-audit.json`: source/import/runtime/toolchain audit and separately
  labeled observation of the QR task's uncommitted host manifest/lock snapshot.
- `png-contract.exe`: non-shipping synthetic test harness, plus generated fixtures.

Primary format references: [W3C PNG specification](https://www.w3.org/TR/png-3/),
[RFC 1950 zlib](https://www.rfc-editor.org/rfc/rfc1950),
[RFC 1951 DEFLATE](https://www.rfc-editor.org/rfc/rfc1951).

## Dependencies and remaining callers

Production module dependencies: **Rust std only; zero external Cargo packages**.
`#![forbid(unsafe_code)]` is enforced. It imports `std::fmt` and implements
`std::error::Error`, and has no OS bindings, C#/Avalonia reference, IPC, installer,
companion executable, external checksum/encoding/cryptographic package or DLL.
The Python/Pillow/zlib oracle is non-shipping test tooling already installed here;
none of it is called by, linked to, or required to run the encoder.

The live QR task's separately owned, uncommitted host snapshot had empty
`dependencies`, `dev-dependencies` and `build-dependencies`; its Cargo lock listed
only package `mcw`. This worker did not create that manifest or claim it as a
published deliverable. Its exact snapshot hashes are in the dependency audit.

**Retained** at the inspected remote baseline, until host integration and broader
migration remove their live callers:

- `QrCode.SaveQrCodeAsync` still calls Avalonia `RenderTargetBitmap` and `rtb.Save`.
  This is the export caller to replace; merely adding the Rust module does not
  remove its current dependency.
- `QrCode.Render`/`DrawQrCodeImage` still render the on-screen receive symbol using
  Avalonia; this track intentionally does not alter the receive UI.
- `Screenshot/Capture.cs` still uses `RenderTargetBitmap` for screenshots.
- `Controls/Spectrum/SpectrumDrawHandler.cs` and `SpectrumDrawOperation.cs` use
  Skia filters, surfaces and leases. `Controls/Rendering/IDrawHandler.cs` and
  `DrawCompositionCustomVisualHandler.cs` use Skia leases for the rendering path.
- `Models/UI/QrCodeReader.cs` still uses `SKBitmap.Decode`, ZXing and QRackers for
  camera scanning. `Models/UI/QrCodeGenerator.cs` still uses the Gma.QrCodeNet
  encoder exposed by QRackers until the QR task finishes its separate migration.
- `MagicalCryptoWallet.Tests/UnitTests/QrDecode/QrCodeDecodingTests.cs` retains the
  managed image/QR decoder as test coverage. VisualPreview screenshot checks
  retain Avalonia bitmap use for Bitcoin P2P, coin selection, fee display,
  history-date and recovery-word verification.
- Fluent package references retain `Avalonia`, `Avalonia.Skia`, `SkiaSharp`,
  `SkiaSharp.NativeAssets.Linux`, and `QRackers`; desktop startup and the rest of
  the Fluent UI retain Avalonia and related packages. No package reference or
  lockfile was removed by this track.

Only the Windows x64 target was installed and executed. The module uses portable
safe std code and explicit byte order, but Linux x64/ARM64 and macOS x64/ARM64
builds/runtime checks remain unverified. No shared toolchain installation was
modified to manufacture a cross-target claim.

## Integration and removal acceptance checks

1. Declare the module in the QR task's existing single `mcw` package, route the
   final bridge/API operations, and preserve empty Cargo dependency sections.
2. Connect actual synthetic QR symbols, retain orientation and the four-module
   margin, preserve save/cancel/filename behavior, and exercise the production
   export adapter with both bit depths. Independently decode the saved PNG and
   recover the intended synthetic QR text.
3. Verify minimum-size snapping and larger display-size exports; test invalid
   matrix/layout/bridge inputs and response-frame bounds. Show actionable export
   failure through the existing UI error path without partial output.
4. Replace the export-only bitmap path and prove no export route reaches Skia or
   Avalonia image encoding. Preserve all other live callers until separately
   migrated; do not claim those complete packages removed.
5. Run the contract/oracle suite on integrated source. Build/test Windows x64,
   Linux x64/ARM64 and macOS x64/ARM64 and audit the actual shipping runtime graph.
   Cross-target and installed-release acceptance remain with the integrator.
6. Update the shared migration ledger with this verified module commit and the
   later adapter/removal evidence. The coordinator owns integration dispatch
   when the QR thread is idle; no integration request was sent on completion.
