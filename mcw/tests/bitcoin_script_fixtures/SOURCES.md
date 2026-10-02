# Bitcoin Script reference provenance

All application Rust in `bitcoin_script.rs` is first-party code written for mcw.
The opcode assignments, Script number/data formats and Bitcoin output templates
are protocol facts. No third-party Script parser or runtime is imported, wrapped,
linked or shipped. The module uses only Rust std and the actually committed
first-party `bitcoin_encoding` address/hex codecs.

## Pinned reference sources

Bitcoin Core v29.0, MIT licensed:

- https://github.com/bitcoin/bitcoin/blob/v29.0/src/script/script.h
- https://github.com/bitcoin/bitcoin/blob/v29.0/src/script/script.cpp
- https://github.com/bitcoin/bitcoin/blob/v29.0/src/script/solver.cpp
- https://github.com/bitcoin/bitcoin/blob/v29.0/src/core_read.cpp
- https://github.com/bitcoin/bitcoin/blob/v29.0/src/core_write.cpp
- https://github.com/bitcoin/bitcoin/blob/v29.0/src/test/data/script_tests.json
- https://github.com/bitcoin/bitcoin/blob/v29.0/src/test/scriptnum_tests.cpp
- https://github.com/bitcoin/bitcoin/blob/v29.0/COPYING

NBitcoin v10.0.13, the retained managed package version, MIT licensed:

- https://github.com/MetacoSA/NBitcoin/blob/v10.0.13/NBitcoin/Script.cs
- https://github.com/MetacoSA/NBitcoin/blob/v10.0.13/NBitcoin/ScriptReader.cs
- https://github.com/MetacoSA/NBitcoin/blob/v10.0.13/NBitcoin/StandardScriptTemplate.cs
- https://github.com/MetacoSA/NBitcoin/blob/v10.0.13/LICENSE

BIP342 opcode-success classification:
https://github.com/bitcoin/bips/blob/master/bip-0342.mediawiki

`manifest.json` records byte counts and SHA-256 digests for every locally read
reference source and generated Core/number corpus. The two adjacent license files
retain the upstream copyright and permission notices. Core's vectors are adapted
as format-only inputs; flags, witness stacks and expected **execution** outcomes
are intentionally excluded from this format checkpoint. No consensus/execution
claim follows from their passing here.

`core_vectors.tsv` holds all 2,414 Script strings from the 1,207 noncomment Core
test rows, paired with the independent Python oracle's byte serialization,
instruction/error summary, non-signature Core asm and lossless Core FormatScript
representation. `numbers.tsv` contains 189 unique creation inputs from Core's
numeric values/offsets plus explicit signed-64-bit endpoints. The oracle uses
Python arbitrary-precision integers and byte slicing independently of Rust.

`application_fixtures.tsv`, `managed_callers.tsv` and `inventory.json` are derived
from immutable Git blobs at the recorded revision, using
`bitcoin_script_inventory.py`. They include only literal source fixtures, never
wallet data or keys. Interpolated/dynamically assembled strings are not counted
as literal fixtures. Matches include tests and comments; the inventory is not a
call-graph or reachability proof.

## Reproduction

Reference downloads belong in ignored `.artifacts/bitcoin-script-evidence/sources`.
Their names/contents are enumerated by `manifest.json`. Download public raw URLs
with an explicit destination, then verify their hashes before regenerating:

```powershell
python mcw/tests/bitcoin_script_reference.py --prepare
python mcw/tests/bitcoin_script_inventory.py
./mcw/tests/bitcoin_script_verify.ps1
```

The verifier holds one shared heavy-build slot, enforces Rust 1.99.0/edition 2024,
and uses ignored harnesses that reference the real committed source files. It
checks formatting/Clippy, debug and optimized tests with overflow checks, and
89,349 independent differential cases. Test-tool executables use the existing
Windows compiler's static CRT to run with the available linker. Those executables
are development evidence only, never application/shipping artifacts. Production
mcw CRT/runtime packaging remains owned by the application integrator.

CoreFormat is the pinned Core test grammar, not Core RPC display asm. Wallet asm
is deliberately hex-oriented and retains the current `OP_10..16` parsing quirk.
Both wallet and RPC display asm can be lossy for nonminimal pushes/negative zero.
Hex and CoreFormat preserve every byte, including malformed tails. Signature
sighash annotations are not generated or interpreted by this format module.
