# Offline codec fixtures

These are public, synthetic reference data. No wallet files or user keys were
read. The Rust tests consume these fixtures without fetching anything or linking
reference code. Python and the BIP reference implementation are development
verification tools only, outside the shipping application.

Retrieved 2026-10-02. Content hashes identify the exact evidence even when the
upstream branch changes:

| Source | SHA-256 of downloaded input | Derived fixture |
| --- | --- | --- |
| [BIP173](https://raw.githubusercontent.com/bitcoin/bips/master/bip-0173.mediawiki) | b19eab06b264bbd15ad7f7d640fa34de8f4a8809fba8a6ecc3e165545b27a543 | bip_vectors.tsv |
| [BIP350](https://raw.githubusercontent.com/bitcoin/bips/master/bip-0350.mediawiki) | 63634b06aa8bae88b31929674736e74964c4598684269ac2b0b140c43a7a0dec | bip_vectors.tsv |
| [Bitcoin Core Base58 vectors](https://raw.githubusercontent.com/bitcoin/bitcoin/master/src/test/data/base58_encode_decode.json) | 20d51011f49339714c28b9244cc5238f4c78bb9206dc8fc61500aed6fc2682ca | base58_vectors.tsv |
| [Bitcoin Core v29.0 valid key I/O vectors](https://raw.githubusercontent.com/bitcoin/bitcoin/v29.0/src/test/data/key_io_valid.json) | cb4978dd01e4d39300597706b131f6c0105ce1fa4c6e8fb20750d09d2ff7370e | core_addresses.tsv |
| [Bitcoin Core v29.0 invalid key I/O vectors](https://raw.githubusercontent.com/bitcoin/bitcoin/v29.0/src/test/data/key_io_invalid.json) | c3ca74ddad7c01faaca7c26063537feb73ff2be03eb7fc83a42a655d2979261d | core_invalid_addresses.tsv |
| [BIP Python reference](https://raw.githubusercontent.com/sipa/bech32/master/ref/python/segwit_addr.py) | 2884ce04a36c8374c4249177cd90e91e545ac73b5c9260307bbeb74ae2e9de0f | External differential checks only; no implementation is copied into mcw |

There are 79 BIP rows: 35 from BIP173 and 44 from BIP350. BIP173's three
original v1+ witness examples remain in the fixture as "witness-obsolete"; the
current address codec must reject their obsolete Bech32 checksums. Generic
Bech32 decoding still recognizes them.

The three out-of-ASCII HRP vectors and the out-of-ASCII checksum vector are
represented as their Unicode code points encoded in UTF-8. A Rust str cannot
contain a lone invalid UTF-8 octet; the codec rejects these non-ASCII values
without lossy conversion. Each input field is hex of its UTF-8 bytes, preserving
leading spaces and case in the fixtures.

Bitcoin Core supplies 21 Base58 pairs, 54 valid public addresses (private-key
vectors are excluded), and 70 invalid strings. Empty inputs are significant
rows, not omitted; the Base58 fixture uses a dash to represent empty fields.
Address-to-script reconstruction occurs only in tests for
independent comparison with Core's expected bytes.

SHA-256 uses the standard algorithm in
[NIST FIPS 180-4](https://csrc.nist.gov/pubs/fips/180-4/upd1/final).
Tests cover the empty, abc, standard 56-byte and 112-byte messages and a million
a bytes. Random binary messages and padding boundaries are separately compared
to Python standard-library hashlib.sha256, including 230 fragmented streams.
The BIP reference is hash checked before differential testing. Base58's
independent reference uses Python arbitrary-precision integers, rather than the
Rust implementation's digit-at-a-time radix loop.

## Rebuilding fixtures

Download the linked files into an ignored reference directory with their listed
filenames, verify each SHA-256 above, then run:

    python mcw/tests/bitcoin_encoding_prepare_fixtures.py .artifacts/bitcoin-encoding-evidence

Review the generated diff before changing fixtures or source hashes. Reference
downloads are not checked in, and normal Rust conformance tests need neither
Python nor internet access.

## Attribution and licenses

BIP173: copyright Pieter Wuille and Greg Maxwell. BIP350: copyright Pieter Wuille.
Both specifications are licensed BSD-2-Clause:

Redistribution and use in source and binary forms, with or without modification,
are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR
ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
(INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

Bitcoin Core test data: copyright (c) 2009-2025 The Bitcoin Core developers.
MIT license:

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
of the Software, and to permit persons to whom the Software is furnished to
do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
