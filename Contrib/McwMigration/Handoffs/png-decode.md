# PNG input codec handoff

State: **verified bounded codec checkpoint**. This adds portable PNG input
decoding under the existing PNG owner. The scanner owner narrowed its assignment
to the QR Model2 decoding leaf and retained its current image/capture boundary.
Production PNG-input cutover requires a separate coordinator-approved scope;
this checkpoint does not change scanning, cameras, UI, bridge dispatch or package
references. No image/camera subsystem migration is claimed.

- Worker: `png`, thread `01a0fc2a-3c9a-7e81-b6e5-d198194b3f77`.
- Implementation and tests commit: `73b233a24f34813811552f3358d8f128457d352a`.
- Starting remote checkpoint: `95fc7c65fedac229f2886a0ccaa8f260253a8478`.
- Publication: normal push to `origin/master`; the shared machine handoff records
  the final documentation commit and verified remote ancestry.
- Existing encoder handoff: [PNG export](png.md).

## Owned files and integration boundary

| Path | Change |
| --- | --- |
| `mcw/src/png.rs` | Exposes the owned `decode` child module; export algorithms unchanged |
| `mcw/src/png/decode.rs` | Safe, bounded, first-party PNG-to-pixel decoder |
| `mcw/tests/png_decode_contract.rs` | Public API, malformed-input and limit tests; independent fixture consumer |
| `mcw/tests/png_decode_verify.py` | Non-shipping Pillow/zlib/original-raster oracle and fixture generator |
| `mcw/tests/png_contract.rs` | Adds the first-party compression module to the existing standalone exporter harness |
| `Contrib/McwMigration/Handoffs/png.md` | Notes the input codec and current module dependency |
| `Contrib/McwMigration/Handoffs/png-decode.md` | This contract and evidence |

The published host at `989cf2a2df22d23837c1aa328e29abfd33c9b9c8` already exposes
first-party `compression` beside `png` in the single `mcw` crate. This worker did
not edit compression, Cargo
manifests, host declarations, handlers, commands, bridge operations, C# adapters,
platform bindings, QR algorithms, capture or the shared migration ledger. No
separate shipping executable or runtime dependency was added. Test executables,
generated images and evidence stay under the checkout's ignored `.artifacts`.

## Stable public API

```rust
let image = png::decode::decode(input, png::decode::DecodeLimits::default())?;
// image.width / image.height: u32
// image.format: PixelFormat::{Gray8, Rgb8, Rgba8}
// image.pixels: Vec<u8>
let channels = image.format.channels();
```

`decode(input: &[u8], limits: DecodeLimits) -> Result<DecodedImage, DecodeError>`
returns only a complete, validated image. Pixels are tightly packed in top-left
row-major order; pixel `(x,y)` starts at `(y * width + x) * channels`. RGBA uses
straight, unassociated alpha. Hidden RGB values are preserved; no premultiplying,
background compositing, color-profile/gamma conversion or EXIF rotation occurs.
Callers own any subsequent alpha/compositing and luminance policy. Neither image
paths nor camera handles enter this API.

| PNG color type | Accepted depths | Output |
| --- | --- | --- |
| 0: grayscale | 1, 2, 4, 8, 16 | Gray8; RGBA8 when tRNS is present |
| 2: RGB | 8, 16 | RGB8; RGBA8 when tRNS is present |
| 3: indexed | 1, 2, 4, 8 | RGB8; RGBA8 when tRNS is present |
| 4: grayscale + alpha | 8, 16 | RGBA8 with replicated gray |
| 6: RGBA | 8, 16 | RGBA8 |

Packed grayscale expands over the full 0..255 range. Sixteen-bit components keep
their high byte; grayscale/RGB tRNS keys compare all original sample bits before
reduction. Palette alpha defaults to 255 for entries omitted by tRNS. Out-of-range
palette indices are errors. Both non-interlaced and all seven Adam7 passes are
supported, including empty passes and independent prior-row/filter state.

Format rules follow the primary [PNG specification](https://www.w3.org/TR/png-3/),
[RFC 1950 zlib](https://www.rfc-editor.org/rfc/rfc1950) and
[RFC 1951 DEFLATE](https://www.rfc-editor.org/rfc/rfc1951). The decoder checks the
signature, every chunk CRC32, alphabetic/reserved chunk-type bits, IHDR validity,
critical chunk order, PLTE/tRNS ordering and lengths, consecutive IDAT chunks,
terminal IEND, all five filters, exact image-derived inflated size and the zlib
Adler32. IDAT may split any zlib byte, including headers, checksum and empty chunks.
CRC-checked ancillary metadata is ignored without decompressing or applying it.
Unknown critical chunks and APNG control/data chunks return explicit errors.

The accepted profile is deliberately stricter than the PNG specification's
recommendation to ignore unused bytes after the completed zlib stream in IDAT:
this API rejects those bytes, preset dictionaries and file bytes after IEND.
Errors expose no provisional pixels. Typed errors distinguish PNG structural,
filter, palette, arithmetic, allocation and quota failures, with the existing
first-party compression error retained as the source of inflate/checksum errors.

## Bounds and dependencies

| Default limit | Value |
| --- | --- |
| Input file | 32 MiB |
| Either dimension | 8,192 pixels |
| Total pixels | 16,777,216 |
| Filtered inflated stream | 160 MiB |
| Requested live allocation capacities | 256 MiB |
| Chunks | 16,384 |
| Inflater work | 2,000,000,000 units |

All limit fields must be nonzero and may be lowered by callers. Dimensions and
format determine exact pass/scanline/output lengths before allocating. The
decoder borrows palette/metadata, then concatenates the validated IDAT bytes,
uses `compression::decode(..., DecodeOptions::new(Format::Zlib))`, unfilters its
buffer in place and allocates the final pixels. Quotas account for actual vector
capacities and leave room for the compression owner's bounded working storage.
Caller-owned input, allocator bookkeeping and transient reallocations are outside
the allocation quota; it is not a process-resident-memory guarantee. The exact
output cap bounds expansion without rejecting legal uniform images above the
compression codec's usual 200:1 ratio threshold.

Production dependencies are Rust standard library and the published first-party
compression module; zero external Cargo/image/zlib packages or native codec DLLs.
`#![forbid(unsafe_code)]` applies. The independently owned compression source used
for this verification has SHA-256
`83488cbc9eb56693c439b2639fcbf14a857dfd7d3d1d3ad3bc8700b812026ab1`.
Pillow, Python and system zlib are test oracles only, already installed, never
called or linked by the production codec.

## Verified evidence

Windows x64, Rust 1.99.0, edition 2024:

- 13 ordinary Rust tests passed in the decoder harness (10 decoder contracts plus
  3 existing encoder/checksum tests); the independent fixture consumer passed
  separately. Coverage includes every truncation/prefix mutation of a valid
  image, bounded random malformed bytes, quotas/work, byte-split IDAT, full-depth
  transparency keys, straight alpha, size/checksum errors and encoder roundtrip.
- **2,727 accepted images matched exact expected pixels**. The independent Python
  writer generated 2,700 combinations: all 15 legal formats, six tiny/asymmetric
  dimensions, both interlace modes, five filter modes and stored/fixed/dynamic
  zlib strategies. Another 22 exercise transparency, one is a 1,024-square
  high-ratio uniform image, and four are existing synthetic repository QR PNGs.
- **18 adversarial/unsupported fixtures were rejected**, covering signatures,
  chunk CRCs/order, palette indices, APNG, filters, size overruns, preset dictionary,
  zlib checksum/trailing bytes and terminal file data.
- Pillow 12.1.1 and zlib 1.3.1.zlib-ng decoded the independently produced images.
  Expected pixels also derive directly from their original sample rasters.
  Pillow's packed-gray tRNS and 16-bit grayscale conversion quirks are handled
  explicitly. Two RGB16 tRNS cases use the original 16-bit raster for exact alpha
  comparison and Pillow for structure, because its 8-bit conversion cannot retain
  the transparency-key precision needed for those cases. Adam7 fixture selection
  uses the standard pass-number grid rather than copying runtime pass tuples.
- Existing encoder regression: 14 ordinary Rust tests passed and all 270 emitted
  PNGs independently decoded with exact pixels/CRC/Adler32/stored-block checks.
- Both harnesses passed Clippy `-D warnings -W clippy::all` and whitespace checks.
  The harnesses locally allow only dead-code for unused cross-component APIs.
  The compression owner corrected its earlier style lint in the latest published
  source; no compression-owner source was changed by this worker.
- `cargo check --locked --offline` passed against the published application host
  plus this codec. Its Cargo lock contains only `mcw`; external dependency sections
  are empty. This compile check does not exercise a production scanner caller.
- Static-CRT decoder test executable imports only native Windows DLLs:
  `api-ms-win-core-synch-l1-2-0.dll`, `bcryptprimitives.dll`, `KERNEL32.dll`,
  `ntdll.dll`, `USERENV.dll`. This is a non-shipping harness audit, not release
  packaging or application dependency-removal evidence.

The optional primary PngSuite HTTPS index returned **HTTP 403**, before corpus
files downloaded. No denial bypass, user-agent workaround or browser download
retry was attempted. PngSuite coverage is **not verified**; the successful default
oracle run reports `pngsuite_downloads: 0`.

Local evidence in the retained publication checkout `.artifacts/mcw-png-decode-publish`:

- `.artifacts/png-decode-verification/fixtures/verification.json`
- `.artifacts/png-decode-verification/encoder-regression/verification.json`
- `.artifacts/png-decode-verification/dependency-audit.json`
- `.artifacts/png-decode-verification/publication.json`

Reproduce in this checkout using the existing shared toolchain and native linker
libraries, one build job and a shared build slot:

```powershell
$env:CARGO_BUILD_JOBS = '1'
$taskRoot = 'C:\Users\user\OneDrive\Documents\ChatGPT\MagicalCryptoWallet'
$env:RUSTUP_HOME = Join-Path $taskRoot '.artifacts\mcw-tools\rustup'
$env:CARGO_HOME = Join-Path $taskRoot '.artifacts\mcw-tools\cargo'
$taskOut = '.artifacts\png-decode-verification'
New-Item -ItemType Directory -Force -Path $taskOut | Out-Null
& "$env:CARGO_HOME\bin\rustc.exe" --edition=2024 --test -D warnings -C target-feature=+crt-static mcw\tests\png_decode_contract.rs -o "$taskOut\png_decode_contract.exe"
& "$taskOut\png_decode_contract.exe" --test-threads=1
python mcw\tests\png_decode_verify.py --decoder-tests "$taskOut\png_decode_contract.exe" --output-dir "$taskOut\fixtures"
& "$env:CARGO_HOME\bin\clippy-driver.exe" --edition=2024 --test --emit=metadata -D warnings -W clippy::all mcw\tests\png_decode_contract.rs --out-dir $taskOut
```

## Deferred acceptance

Only Windows x64 was built/executed. Linux x64/ARM64 and macOS x64/ARM64 remain
unverified. A separately approved caller cutover must choose documented limits,
alpha/luminance behavior and a failure/cancellation path, then exercise actual
synthetic scanner inputs through the real host. Current Skia image acquisition,
Avalonia and FlashCap/camera paths remain transitional. This codec changes no
package references and proves no QR text recovery or camera replacement. The
coordinator owns integration dispatch; no request to broaden or cut over the
active scanner/QR host was sent on completion.
