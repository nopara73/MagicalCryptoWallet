# PSBT reference data

The 94 rows in `psbt_vectors.tsv` are taken from Ava Chow's [BIP174](https://github.com/bitcoin/bips/blob/3a10b5b5f0a7586df8928d580a3009744ebb2079/bip-0174.mediawiki) and [BIP370](https://github.com/bitcoin/bips/blob/3a10b5b5f0a7586df8928d580a3009744ebb2079/bip-0370.mediawiki), at bitcoin/bips commit `3a10b5b5f0a7586df8928d580a3009744ebb2079`. Each BIP is licensed under BSD-2-Clause. The fixture header records the SHA256 of each downloaded source. The data is public, synthetic reference data; it is unrelated to any user's wallet.

BIP174 contributes 20 invalid containers, 10 valid named containers, 13 valid role examples, and four containers that fail signer checks. BIP370 contributes 24 invalid containers and 23 valid containers, including nine locktime examples (one with incompatible required locktime types). Containers with incompatible locktimes remain parseable; unsigned transaction construction returns an error. The four signer-only examples remain parseable because this module checks the format and does not establish script, UTXO, or signature validity.

`psbt_vectors.py` extracts the fixture from local copies of those two pinned source files. It cross-checks every published Base64 string against both its published hex bytes and Python standard library `base64.b64encode`/`b64decode(validate=True)`. It also retains the second unnamed locktime vector after BIP370's last named time-locktime example. The fixtures are checked in; Python and the network are not required to run the Rust tests, and neither is a shipping dependency.

`psbt_managed_reference.ps1` uses the already cached NBitcoin 10.0.13 assembly to parse all 94 rows, then captures its binary/Base64 exports of container-valid examples in `psbt_managed_vectors.tsv`. This yields 47 reference exports, which the Rust tests parse and reproduce byte for byte. The assembly SHA256 is `ebb7e5548fe1325514289528e67b2ee0e24b3bfecb75ed4067c44ea99a202167`. No package is restored or installed, the managed assembly is not a dependency of the Rust tests, and the Rust implementation never loads it. The test fixtures inherit the original BIP data attribution and license below.

To regenerate from the pinned sources in a workspace directory:

```powershell
New-Item -ItemType Directory -Path .artifacts/psbt-evidence -Force | Out-Null
Invoke-WebRequest 'https://raw.githubusercontent.com/bitcoin/bips/3a10b5b5f0a7586df8928d580a3009744ebb2079/bip-0174.mediawiki' -OutFile .artifacts/psbt-evidence/bip-0174.mediawiki
Invoke-WebRequest 'https://raw.githubusercontent.com/bitcoin/bips/3a10b5b5f0a7586df8928d580a3009744ebb2079/bip-0370.mediawiki' -OutFile .artifacts/psbt-evidence/bip-0370.mediawiki
python mcw/tests/psbt_vectors.py .artifacts/psbt-evidence mcw/tests/psbt_vectors.tsv
```

For the reference vector data, the BSD-2-Clause terms apply:

Copyright: Ava Chow, author of BIP174 and BIP370.

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
