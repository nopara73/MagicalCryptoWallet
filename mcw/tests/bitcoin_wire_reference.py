#!/usr/bin/env python3
"""Regenerate wire fixtures with unmodified Bitcoin Core v29.0 reference code.

Test tooling only: Python and downloaded Core sources are not shipped by mcw.
Downloads go directly to the ignored task workspace; expected SHA256 pins prevent
silent reference changes. No network node, wallet files, keys, or signing are used.
"""

import hashlib
import importlib
import io
import json
from pathlib import Path
import random
import re
import sys
import urllib.request


ROOT = Path(__file__).resolve().parents[2]
REFERENCE = ROOT / ".artifacts/bitcoin-wire/reference"
OUTPUT = ROOT / "mcw/tests/bitcoin_wire_fixtures"
BASE = "https://raw.githubusercontent.com/bitcoin/bitcoin/v29.0/"
SOURCES = {
    "src/test/data/tx_valid.json": "a85d479081d6fd93188377e27fdfa099fa1a6aefdae565d0f473003c855d1e90",
    "src/test/data/tx_invalid.json": "0c02ce44ff3a880458f9569a25589315a07f924fcaadac828613d4615776ca52",
    "src/primitives/transaction.h": "57769818190672703fb5e1159785a0f7c5b050987b591580450a72eb3fcbba05",
    "test/functional/test_framework/messages.py": "37cdc47790abb64ad909beae0a12ffba99e5123b429a2879ce448414d8139c00",
    "test/functional/test_framework/crypto/siphash.py": "9ebc188ce51969a9757305939f295bbb2ba9a2f79dec19176eb79990b3df34a2",
    "test/functional/test_framework/util.py": "0f166c3d800ad77a348e70f57a4bf36d0a4a6e8d8bceeb249f62977287ae2b5e",
    # These utility imports do not implement transaction or hashing behavior.
    "test/functional/test_framework/coverage.py": "8888acf9f9c7ef09ec80c23f53c193d0658b86abfa2e7896ace27daa4928fca8",
    "test/functional/test_framework/authproxy.py": "cf945e49b309a11d1fc24bff88f6beb7ff95bb669f4d0676d0c69bcda6e29cae",
}


def prepare_reference():
    REFERENCE.mkdir(parents=True, exist_ok=True)
    records = []
    for source, expected in SOURCES.items():
        if "test_framework/" in source:
            destination = REFERENCE / source[source.index("test_framework/"):]
        else:
            destination = REFERENCE / Path(source).name
        destination.parent.mkdir(parents=True, exist_ok=True)
        if not destination.exists():
            old = REFERENCE / Path(source).name
            if old.exists():
                destination.write_bytes(old.read_bytes())
            else:
                with urllib.request.urlopen(BASE + source, timeout=30) as response:
                    destination.write_bytes(response.read())
        digest = hashlib.sha256(destination.read_bytes()).hexdigest()
        if expected is not None and expected != digest:
            raise RuntimeError(f"Reference digest mismatch: {source}: {digest}")
        records.append({"url": BASE + source, "sha256": digest})
    sys.path.insert(0, str(REFERENCE))
    return importlib.import_module("test_framework.messages"), records


def fingerprint(tx):
    parts = [str(tx.version), str(tx.nLockTime)]
    stacks = tx.wit.vtxinwit
    for index, entry in enumerate(tx.vin):
        witness = stacks[index].scriptWitness.stack if index < len(stacks) else []
        parts.append("i:" + ":".join([
            entry.prevout.hash.to_bytes(32, "little").hex(), str(entry.prevout.n),
            str(entry.nSequence), entry.scriptSig.hex(), ",".join(x.hex() for x in witness),
        ]))
    for entry in tx.vout:
        parts.append(f"o:{entry.nValue}:{entry.scriptPubKey.hex()}")
    return hashlib.sha256("\n".join(parts).encode("ascii")).hexdigest()


def row(name, mode, tx):
    wire = tx.serialize_with_witness()
    stripped = tx.serialize_without_witness()
    tx.calc_sha256()
    weight = len(stripped) * 3 + len(wire)
    return "\t".join(map(str, [
        name, mode, wire.hex(), stripped.hex(), tx.hash, tx.getwtxid(),
        weight, (weight + 3) // 4, tx.version, tx.nLockTime,
        len(tx.vin), len(tx.vout), fingerprint(tx),
    ]))


def parsed_row(core, name, raw):
    tx = core.CTransaction()
    stream = io.BytesIO(raw)
    tx.deserialize(stream)
    if stream.tell() != len(raw) or tx.serialize_with_witness() != raw:
        # Python reference is deliberately permissive. Only byte-exact canonical
        # transactions enter the differential corpus; rejection is tested in Rust.
        raise ValueError("reference did not exactly round trip")
    return row(name, "witness", tx)


def synthetic(core, rng, input_count, output_count, script_len=None, witness_items=None):
    tx = core.CTransaction()
    tx.version = rng.getrandbits(32)
    tx.nLockTime = rng.getrandbits(32)
    for index in range(input_count):
        script = rng.randbytes(script_len if script_len is not None else rng.randrange(0, 40))
        tx.vin.append(core.CTxIn(core.COutPoint(rng.getrandbits(256), rng.getrandbits(32)), script, rng.getrandbits(32)))
        wit = core.CTxInWitness()
        count = witness_items if witness_items is not None else rng.randrange(0, 4)
        wit.scriptWitness.stack = [rng.randbytes(rng.randrange(0, 40)) for _ in range(count)]
        tx.wit.vtxinwit.append(wit)
    for index in range(output_count):
        value = [-2**63, 2**63 - 1, -1, 0, 21_000_000 * 100_000_000][index % 5]
        script = rng.randbytes(script_len if script_len is not None else rng.randrange(0, 40))
        tx.vout.append(core.CTxOut(value, script))
    return tx


def main():
    core, reference_sources = prepare_reference()
    rows = []
    skipped = []
    groups = {}
    for file in ("tx_valid.json", "tx_invalid.json"):
        count = 0
        for index, entry in enumerate(json.loads((REFERENCE / file).read_text(encoding="utf-8"))):
            if len(entry) != 3 or not isinstance(entry[1], str):
                continue
            try:
                rows.append(parsed_row(core, f"core-{file}-{index}", bytes.fromhex(entry[1])))
                count += 1
            except (ValueError, IndexError, OverflowError, AssertionError) as error:
                skipped.append({"source": file, "index": index, "reason": type(error).__name__})
        groups[file] = count
    # These are retained test-source literals, never a user's wallet/database.
    source = ROOT / "MagicalCryptoWallet.Tests/UnitTests/Transactions/AllTransactionStoreTests.cs"
    source_bytes = source.read_bytes()
    seen = set()
    for match in re.finditer(r'Transaction\.Parse\("([0-9a-fA-F]+)"', source_bytes.decode("utf-8-sig")):
        raw = bytes.fromhex(match.group(1))
        if raw in seen:
            continue
        seen.add(raw)
        rows.append(parsed_row(core, f"retained-store-{len(seen)}", raw))
    groups["retained-store"] = len(seen)
    rng = random.Random(0x4D435757495245)
    for index in range(128):
        tx = synthetic(core, rng, rng.randrange(1, 5), rng.randrange(0, 5))
        rows.append(row(f"synthetic-{index}", "witness", tx))
    boundaries = [(0, 0, 0, 0), (0, 2, 0, 0), (1, 0, 0, 0),
                  (252, 1, 0, 0), (253, 1, 0, 0), (1, 252, 0, 0),
                  (1, 253, 0, 0), (1, 1, 252, 0), (1, 1, 253, 0),
                  (1, 1, 65535, 0), (1, 1, 65536, 0),
                  (1, 1, 0, 252), (1, 1, 0, 253)]
    for index, args in enumerate(boundaries):
        tx = synthetic(core, rng, *args)
        rows.append(row(f"boundary-{index}", "legacy" if args[0] == 0 else "witness", tx))
    groups["synthetic"] = 128 + len(boundaries)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    header = "# name\tmode\twire\tstripped\ttxid\twtxid\tweight\tvsize\tversion-bits\tlocktime\tinputs\toutputs\tfield-sha256\n"
    encoded = (header + "\n".join(rows) + "\n").encode("ascii")
    (OUTPUT / "reference.tsv").write_bytes(encoded)
    manifest = {
        "reference": "Bitcoin Core v29.0 (MIT); original messages.py plus original stdlib-only utility imports",
        "references": reference_sources,
        "retained_source": {"path": str(source.relative_to(ROOT)).replace("\\", "/"), "sha256": hashlib.sha256(source_bytes).hexdigest()},
        "groups": groups, "total": len(rows), "skipped_non_exact_reference_rows": skipped,
        "fixture_sha256": hashlib.sha256(encoded).hexdigest(),
        "scope": "Wire structure/bytes/hashes/sizes only. Core tx_invalid rows can be structurally valid; consensus validity is not asserted.",
    }
    (OUTPUT / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"groups": groups, "total": len(rows), "skipped": len(skipped), "fixture_sha256": manifest["fixture_sha256"]}))


if __name__ == "__main__":
    main()
