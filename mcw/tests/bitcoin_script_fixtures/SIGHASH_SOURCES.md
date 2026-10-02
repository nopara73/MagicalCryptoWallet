# Signature hash reference provenance

`script_service/sighash.rs` is first-party Rust written for mcw, using the actual
first-party bitcoin_wire, bitcoin_script and bitcoin_encoding APIs. There is no
NBitcoin/libbitcoin/crate/native companion dependency. It computes hash preimages
and digests, never signature verification or spendability.

- `legacy_sighash.tsv`: all 500 Bitcoin Core v29.0
  [sighash.json](https://github.com/bitcoin/bitcoin/blob/v29.0/src/test/data/sighash.json)
  expected hashes. The adjacent BITCOIN_CORE_LICENSE.txt retains MIT copyright
  and permission notices. Source expectations use uint256 display order; derived
  fixture digests are explicitly reversed into raw SHA output order.
- `segwit_sighash.tsv`: ten published
  [BIP143](https://github.com/bitcoin/bips/blob/master/bip-0143.mediawiki)
  preimages/digests bound to the actual matching raw transaction/input, scriptCode
  and amount from those examples. BIP143 is public domain.
- `taproot_sighash.tsv`: seven actual epoch-prefixed messages and digests from
  [BIP341 wallet vectors](https://github.com/bitcoin/bips/blob/master/bip-0341/wallet-test-vectors.json).
  [BIP341](https://github.com/bitcoin/bips/blob/master/bip-0341.mediawiki) lists
  authors Pieter Wuille, Jonas Nick and Anthony Towns and BSD-3-Clause licensing.
  Only public transaction/prevout/message/hash fields are retained; published
  private keys, signing parameters and signatures are not copied into this corpus.

`sighash_manifest.json` records SHA-256 digests of all source documents and
derived TSVs, plus exact vector counts. `bitcoin_script_sighash_reference.py`
implements a separate byte decoder/serializer and hashing oracle with Python's
arbitrary-precision integers and hashlib. It checks all published expected
digests/messages before writing fixtures. The oracle never calls Rust to derive
an expectation. Master-branch BIP source versions are pinned by these hashes.

Independent differential testing adds 3,600 randomized legacy/BIP143/BIP341
checks over 1,200 synthetic transactions. It covers ANYONECANPAY, ALL/NONE/SINGLE,
arbitrary 32-bit legacy hash types, missing SINGLE outputs, scriptCode separator
bytes, ordered prevout binding, annex hashes and BIP342 leaf/separator extensions.

Reproduction uses the public raw source URLs in the ignored sources directory,
the shared already-installed Rust toolchain, one heavy-build slot and single-job
builds:

```powershell
python mcw/tests/bitcoin_script_sighash_reference.py --prepare
./mcw/tests/bitcoin_script_sighash_verify.ps1
```

Only the nine debug/optimized tests and differential hashing checks are verified
by this checkpoint. Script execution, primitive ECDSA/BIP340/point commitments,
actual bridge/caller migration and native five-target shipping acceptance remain
separate work. Static CRT linking applies only to ignored Windows test tools.
