# Normative Brotli format data

`dictionary.bin` contains the exact 122784 protocol dictionary bytes specified
in RFC7932 Appendix A, with normative CRC32 `5136cb04`. `tables.rs` contains the
three context lookup tables from section 7.1, dictionary offsets/depths from
Appendix A, and the 121 word transformations from Appendix B. These are format
data, embedded into the one mcw executable, not a third-party decoder/library,
runtime, helper executable, or external compression dependency.

Source: https://www.rfc-editor.org/rfc/rfc7932.txt (July 2016), by J. Alakuijala
and Z. Szabadka. Copyright (c) 2016 IETF Trust and the persons identified as the
document authors. RFC legal notice and Simplified BSD license are retained in
`RFC-DATA-LICENSE`. `manifest.json` records source/asset SHA256 plus normative
dictionary, lookup-table and transform CRCs. No third-party implementation is
copied; all decoder logic is independently written first-party Rust.

Regeneration uses only the Python standard library:

```powershell
python mcw/tests/compression_brotli_tables.py --rfc .artifacts/compression/rfc7932.txt
```

Download the public RFC directly into the stated ignored evidence path first.
The generator checks all normative lengths/CRCs before writing/using data.
Dictionary entries include arbitrary octets and multilingual text; they are
format-defined, never inferred from local/private wallet or user data.
