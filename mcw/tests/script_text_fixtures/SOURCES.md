# Legacy client Script text reference

The implementation is first-party Rust/std-only and reuses the published
`bitcoin_script` format codec. No NBitcoin source is copied into the implementation.

The development oracle references the application's already-retained NBitcoin
10.0.13 `net10.0` assembly, SHA-256
`ebb7e5548fe1325514289528e67b2ee0e24b3bfecb75ed4067c44ea99a202167`.
Its NuGet metadata identifies repository commit
`bd666454562c12155210fac65b07c70289dcecc1`. The oracle uses `new Script(string)`
and `new Script(byte[]).ToString()` directly. It is never linked or shipped with
the Rust application, and it adds no runtime package reference.

Relevant pinned primary sources:

- [NBitcoin Script string constructor and display](https://github.com/MetacoSA/NBitcoin/blob/v10.0.13/NBitcoin/Script.cs)
- [NBitcoin opcode/text reader and malformed-push display](https://github.com/MetacoSA/NBitcoin/blob/v10.0.13/NBitcoin/ScriptReader.cs)
- [NBitcoin net10 hex decoder](https://github.com/MetacoSA/NBitcoin/blob/v10.0.13/NBitcoin/DataEncoders/HexEncoder.cs)
- [NBitcoin license](https://github.com/MetacoSA/NBitcoin/blob/v10.0.13/LICENSE)

The retained MIT notice is in `NBitcoin-MIT.txt`. Fixtures are synthetic generated
inputs and independently captured reference results, not library implementation
code. `manifest.json` records the exact assembly, oracle/generator, fixture hashes
and counts. Hex fields keep whitespace, Unicode, empty values and malformed raw
bytes unambiguous; a final `.` column avoids trailing whitespace in Git patches.

Compatibility includes permissive `OP_UNKNOWN(0xNN` prefixes and ignored suffixes,
all six ASCII separators including vertical tab, .NET Unicode boundary trimming,
hex-oriented number words/aliases, nonminimal push display, and the final `0`
produced for a truncated push or length header. Strict parser/template APIs and
lossless byte formats remain separate and unchanged.

The published codec's explicit Script/asm resource bounds still apply. The
production service additionally has the existing version-one bridge's 1 MiB
frame bound (16-byte header); oversized requests/results fail explicitly. These
checks do not claim unrestricted global NBitcoin compatibility, execution,
signing, package retirement, or acceptance on unavailable native targets.
