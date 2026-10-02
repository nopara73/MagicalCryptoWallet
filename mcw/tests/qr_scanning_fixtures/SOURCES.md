# QR scanner fixtures

`golden.tsv` contains immutable format data from the independent Nayuki
QR-Code-generator v1.8.0 Python encoder. The oracle is not included in the
application or Cargo graph. Source:
https://github.com/nayuki/QR-Code-generator/blob/v1.8.0/python/qrcodegen.py
SHA256: `b089855caf16185c61421ea4927c1b213cf9468940d71fa8ab11ef83662dcc84`.

Rows contain size, exact text, then row-major 0/1 modules without quiet zone.
These three symbols exercise versions 1, 7 and 40, level Q, mask 0, and byte
content `MCWQR`. Decoder implementation does not generate the expected symbols.

Full independent matrix, raster, perspective and content corpora are generated
by the uniquely named `qr_scanning_*_reference.py` development scripts into the
ignored worker evidence directory. No existing camera is used or activated.
The retained repository images are test fixtures with expected strings from
the existing `QrCodeDecodingTests`; Pillow normalization is a dev-only oracle.

The initial matrix/raster implementation was reused from this project's earlier
first-party Rust wallet rewrite. Source paths were
`src/platform/windows/qr_decode.rs` and `qr_image.rs` in that historical checkout;
initial SHA256s were `675aa1a972b67fbd890206006cf598b5b9f3626685f61c4b16c9f015b1a2dab1`
and `a9e0b5c0a0b5bef0e52ca17f259568f7f31c9733e890232d61ddf8d385026898`.
All claims in the new handoff are tied to tests rerun against the new source.
