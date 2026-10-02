# Vector and implementation provenance

`wallet_hashes.rs` is original first-party Rust implementing the algorithms from
their mathematical specifications. No crate, OS crypto library, C implementation,
runtime, executable, or third-party hash implementation is copied or wrapped.
SHA256 is the committed sibling `bitcoin_encoding::Sha256`, first published in
`6365f3244d23b801c0bb36581967caad8aae968e`; it is never duplicated here.
The code uses the repository MIT license. Offline fixtures are vector facts,
not reference implementations. Source URLs, downloaded lengths, exact SHA256
hashes, NIST ZIP member hashes, and the Trezor revision are in `manifest.json`.

Sources:

- [RIPEMD160 designers' specification](https://homes.esat.kuleuven.be/~bosselae/ripemd/rmd160.txt)
  and [published vectors](https://homes.esat.kuleuven.be/~bosselae/ripemd160.html):
  eight short fixtures and the ninth (million `a`) conformance test. Algorithm
  constants and public round permutations follow the specification; no reference
  C implementation is incorporated.
- [FIPS 180-4](https://csrc.nist.gov/files/pubs/fips/180-4/final/docs/fips180-4.pdf):
  SHA512 IV, round constants, padding, and byte order. [NIST byte-oriented CAVP
  vectors](https://csrc.nist.gov/CSRC/media/Projects/Cryptographic-Algorithm-Validation-Program/documents/shs/shabytetestvectors.zip):
  every SHA512 ShortMsg (129) and LongMsg (128) example. Passing these is informal
  conformance evidence, not FIPS validation. The million-`a` value is also
  published in [RFC 6234](https://www.rfc-editor.org/rfc/rfc6234).
- [RFC 4231](https://www.rfc-editor.org/rfc/rfc4231): all seven HMAC cases for each
  PRF, including the explicitly truncated case 5. Verification APIs still demand
  the complete MAC. [RFC 2104](https://www.rfc-editor.org/rfc/rfc2104) defines HMAC;
  [RFC 8018](https://www.rfc-editor.org/rfc/rfc8018) defines PBKDF2 and its big-endian
  four-byte counter. No RFC reference code is incorporated.
- [RFC 7914 section 11](https://www.rfc-editor.org/rfc/rfc7914#section-11): both
  PBKDF2-HMAC-SHA256 fixtures, including 80,000 iterations and two output blocks.
- [BIP39](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki) and its
  [author-maintained Trezor vectors](https://github.com/trezor/python-mnemonic/blob/master/vectors.json):
  24 examples in each of 12 languages. Fixture generation supplies UTF-8 NFKD
  bytes using Python `unicodedata`, with `mnemonicTREZOR`, 2048 rounds, and 64-byte
  output. The primitive does not provide Unicode normalization or mnemonic
  parsing, generation, validation, or wallet key derivation. Preserve the supplied
  `TREZOR-LICENSE` (MIT, copyright Pavol Rusnak) with these vector fixtures.
- [SLIP21 public example](https://github.com/satoshilabs/slips/blob/master/slip-0021.md):
  conformance covers HMAC inputs and expected node slices without a production
  node/key API. The existing OwnershipProofTest's [pinned SLIP19 example](https://github.com/satoshilabs/slips/blob/846a0a6c1dfc29f8b90fd90a9309b1174b7d91e8/slip-0019.md#test-vector-1-p2wpkh)
  supplies script bytes and the expected HMAC-based identifier. No signature or
  curve operation is implemented by these tests.

The Python differential reference uses only standard-library `hashlib` and `hmac`
(on this host backed by OpenSSL). Python/OpenSSL and the ignored static-CRT Windows
test executables are development verification tools, never mcw dependencies or
shipping artifacts. Downloaded originals and full synthetic inputs are kept in
the ignored `.artifacts/wallet-hashes-evidence` directory.

Regenerate fixtures with `python mcw/tests/wallet_hashes_prepare_fixtures.py
--trezor-commit <manifest revision>`. `--offline` reuses the downloaded originals.
The regular verification command reads the checked-in fixtures without network
access, validates their SHA256, and never fetches reference implementation code.
