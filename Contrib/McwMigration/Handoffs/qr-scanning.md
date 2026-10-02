# Bounded QR image/text decoding migration

Status: scanner-owned implementation and integration patches are verified and
ready for coordinated host incorporation (`checkpoint_only=true`).
The production caller must be incorporated together with host registration.
This handoff does not claim a shipping camera replacement or dependency removal.

## Scope and ownership

This assignment replaces Model 2 raster detection, sampling, error correction,
segment parsing, and text decoding in the retained `QrCodeReader` leaf. The Rust
implementation lives in `mcw/src/scan_service/`; the small managed pixel adapter
lives in `MagicalCryptoWallet/Mcw/Scanning/McwQrDecoder.cs`.

The concrete caller patch changes only
`MagicalCryptoWallet.Fluent/Models/UI/QrCodeReader.cs`. It removes its ZXing
decoder and sends grayscale pixels through the existing `IMcwApplicationServices`
connection. FlashCap device selection/acquisition, Skia image acquisition,
Avalonia previews, the observable result shape, and wallet payment/address
validation remain with their existing owners. Captured bytes and decoded text
are never written to a file by the runtime. The callback awaits the Rust response
while FlashCap owns its image scope, then emits the existing preview/result.

The earlier broad camera/service draft is preserved only in ignored worker
artifacts and is excluded from this change. No camera, UI backend, platform, or
wallet engine replacement is included.

## Integration patches

`Contrib/McwMigration/Patches/qr-scanning-caller.patch` contains the complete
caller change. `qr-scanning-host.patch` adds the Rust module and a connection-owned
`wire::Runtime` in the existing application host. Both patches must be
incorporated together by the shared host owner. Their initial base is
`8a438c9737926887a1491e45bb0d82a718ca0fc0`. The actual-host test applied them
successfully to shared revision `6e8bf58cbf1c60fa95729ea0ea317bc6d330ba95`
for the privacy-audit rerun, using that revision's current host entry point.
Their scoped Git attributes preserve LF on Windows. The verifier can check both
patches against the current retained sources with `-CheckPatchesOnly`, without
acquiring a build slot or rebuilding the unchanged decoder.

The host reader must register scanner requests immediately after `Frame::read`
and before the bounded dispatcher queue. It must deliver matching cancellation
directly through the scanner reader handle while decode is in progress, and call
`Reader::disconnected()` on EOF/failure. The dispatcher calls `Runtime::handle`,
reaps expired uploads on each loop, and drops the owner on connection exit. This
keeps the existing private transport, lifecycle, framing, and other operation
owners. A decoder fallback is not provided.

## Wire contract

All integers use little endian. Every request begins with `u16 version=1`,
`u16 reserved=0`, and a nonzero client-assigned `u64 transfer_id`.

| Operation | Payload after the 12-byte header | Behavior |
| --- | --- | --- |
| `0x1300 BEGIN` | `u32 width, height, stride` | Reserve one bounded grayscale upload. |
| `0x1301 APPEND` | `u32 offset`, then 1–262144 bytes | Append exactly at the current offset. |
| `0x1302 FINISH` | Empty | Remove the upload, detect/sample/decode once, and return exact text or no symbol. |
| `0x1303 ABORT` | Empty | Idempotently release the upload. |

The image is top-row-first Gray8. Width/height are 1–4096, stride is at least
width and at most 16384, and total pixels/bytes are at most 16777216. Exact
`stride * height` bytes are required; no downsampling is performed. Four uploads
are permitted per connection, each expiring ten seconds after BEGIN. The managed
adapter aborts failed/cancelled transfers; expiry and connection-owner cleanup
are additional boundaries. Decode has a cooperative two-second deadline measured
from handler execution and reader cancellation. These do not establish an
end-to-end cancellation or cleanup bound under dispatcher queue saturation.
Runtime limits are independent of the bridge's one-MiB
frame limit because pixel uploads use bounded chunks.

ACK/no-symbol is exactly `[1,0,0,0]`. A successful FINISH has a 16-byte header:
`u16 version=1, u8 status=1, u8 reserved=0, u8 qr_version, u8 level,
u16 corrected_symbols, u8 structured_index, u8 structured_total,
u8 structured_parity, u8 structured_flag, u32 utf8_length`, followed by exact
UTF-8 text. Levels are 0=L, 1=M, 2=Q, 3=H. Structured metadata is zero when absent.
The managed adapter validates the entire response before returning it.

## Decoder behavior

Model 2 versions 1–40, all error correction levels/masks, format/version BCH,
Reed–Solomon correction, rotation/mirror handling, global/local thresholding,
finder sampling, and bounded perspective alignment are implemented in safe
first-party Rust. Numeric, alphanumeric, byte, Kanji, Hanzi, ECI, FNC1, and
structured-append metadata are supported. Empty text, NUL, BOM, whitespace, and
Unicode remain exact. This leaf does not assemble several camera scans into a
structured-append message or interpret text as a payment instruction.

ECI character maps are generated data from Python's development-only codecs;
the runtime does not depend on Python, a codec library, or an external Cargo
crate. Unsupported explicit ECIs are rejected. Candidate decode failures cannot
substitute guessed payment text. Multiple different valid symbols are rejected
as ambiguous. Error diagnostics do not echo image bytes or decoded text. Rust
`Decoded::Debug` and C# `QrDecodedText.ToString()` redact text while retaining
format metadata; marker tests cover ordinary/nested Rust formatting and managed
record/object/clone formatting without changing the returned payload. The public
text fields remain available to callers, so this does not claim that arbitrary
reflection-based serializers or application logging redact those fields.

## Verification and remaining obligations

Final source-bound evidence is recorded in the ignored worker directory
`.artifacts/qr-scanning/`. `mcw/tests/qr_scanning_verify.ps1` runs debug and
optimized conformance tests plus strict Clippy and builds a development-only
oracle driver. Independent reference scripts exercise all versions/levels/masks,
damaged symbols, raster transforms, modes/Unicode, perspective, and the five
retained repository QR fixtures. Fixture provenance and oracle hashes are in
`mcw/tests/qr_scanning_fixtures/SOURCES.md`.

Final Windows x64 development verification passed strict Clippy, 11 debug and
11 optimized tests, 1817 independent matrix cases (591 corrected symbols),
74 raster cases, 42 content/mode/Unicode cases, 12 perspective cases, and all
five retained repository images. Every independent corpus ran against oracle
binary SHA256 `fdf8f43fe35dde23a1efbc1d5e4c7ab59099be3f620d81454ac708bdaeb202a6`.
`qr-scanning-evidence.json` preserves the source hashes and bounded results.

`qr_scanning_managed_verify.ps1` compiles the actual patched `QrCodeReader`,
adapter, and core interface using existing package assemblies, then tests them
against the actual Rust runtime/Frame. It also compiles a development-only copy
of the patched actual host and native Windows child-ownership code and exercises
the actual `ManagedApplicationHost.Connect/RequestAsync/Dispose` transport.
Both managed paths passed all 42 generated symbols and all five original image
fixtures through the actual patched capture-decode leaf. The direct Frame path
also passed a 4096-square chunked upload, malformed image/reply rejection, and
reader cancellation synchronized to the fixture's FINISH-dispatch marker. Its
request-return latency is a single observation recorded in the evidence, not a
general decoder/host bound. The actual managed transport/host path passed caller
cancellation and subsequent decoding after a 40 ms delay; that test is not
synchronized to in-progress FINISH and does not prove mid-decode cancellation.
Its unused `BindTermination` parameter has a test-only type shim; full wallet
termination behavior is not claimed. Generated images are synthetic, and no
camera is opened. The portable test tools use static CRT solely for development;
they are not Cargo targets or shipping executables and do not prove audited
release imports or five-target packaging.

The shared owner must still apply/verify the caller and host patches in its
current checkout, run the host's required CI/package checks, and confirm actual
production execution before setting `production_integrated` or
`old_implementation_retired`. Camera hardware, Linux/ARM/macOS execution, and
packaged release runtime/import verification remain unverified here.

Shared acceptance gates also remain for saturated-queue cancellation/EOF
delivery, adversarial reuse of completed request IDs, and disconnect during an
in-progress decode. The bounded reader's synchronous queue send can block before
it reads CANCEL or EOF. The ordinary managed caller allocates unique monotonic
IDs and drains late replies; the scanner registry rejects active duplicates but
does not enforce completed-ID non-reuse. Do not generalize the unsaturated
fixture observations beyond that caller contract. Static-CRT development tools
remain distinct from shipping rebuilt-stdlib and package verification.

`QRackers` cannot be removed by this bounded task: its single assembly also
contains the retained FlashCap capture implementation. Its production package
references and lock entries therefore remain, and `dependency_removed` is false.
