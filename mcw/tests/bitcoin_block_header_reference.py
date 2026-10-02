"""Prepare synthetic header hashes with the existing hash-pinned Core reference.

Offline test tooling only. Nothing here is linked or shipped in the application.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
from pathlib import Path
import random
import bitcoin_block_reference as reference


def prepare(references: Path, evidence: Path) -> None:
    fixtures = Path(__file__).with_name("bitcoin_block_fixtures")
    manifest_path = fixtures / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8-sig"))
    core = reference.core_reference(references, manifest["sources"])
    generator = random.Random(0x0E00)
    vectors = [("zero", bytes(80)), ("ones", bytes([255]) * 80),
               ("ordered", bytes(range(80)))]
    vectors.extend((f"random-{index}", generator.randbytes(80)) for index in range(253))
    rows = ["# Synthetic inputs only; id\theader_hex\thash_raw\thash_display"]
    for name, raw in vectors:
        header = core["CBlockHeader"]()
        header.deserialize(io.BytesIO(raw))
        assert header.serialize() == raw
        display = header.hash_hex
        digest = bytes.fromhex(display)[::-1]
        assert digest == hashlib.sha256(hashlib.sha256(raw).digest()).digest()
        rows.append("\t".join((name, raw.hex(), digest.hex(), display)))
    content = ("\n".join(rows) + "\n").encode()
    (fixtures / "headers.tsv").write_bytes(content)
    fixture_hash = hashlib.sha256(content).hexdigest()
    manifest["files"]["headers.tsv"] = fixture_hash
    manifest["header_vectors"] = len(vectors)
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8", newline="\n")
    evidence.mkdir(parents=True, exist_ok=True)
    report = {"vectors": len(vectors), "seed": "0x0e00", "synthetic_only": True,
              "fixture_sha256": fixture_hash,
              "core_messages_sha256": manifest["sources"]["test/functional/test_framework/messages.py"]["sha256"],
              "reference": "Bitcoin Core v30.0 unchanged CBlockHeader plus independent hashlib"}
    (evidence / "header-reference.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--references", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    arguments = parser.parse_args()
    prepare(arguments.references, arguments.evidence)
