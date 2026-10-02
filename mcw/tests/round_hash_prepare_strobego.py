"""Derive AD/metaAD/PRF cases from vendored, unmodified published STROBEgo vectors.

Only the three hash operations ship. Unsupported upstream operations supply
test-only checkpoints; they are never implemented or dispatched by this leaf.
"""
import argparse
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--check", action="store_true")
parser.add_argument("--source", type=Path, default=root / "ThirdParty/WabiSabi/csharp/WabiSabi.Tests/Data/StrobeTestVectors.json")
parser.add_argument("--destination", type=Path, default=Path(__file__).with_name("round_hash_vectors") / "strobego.tsv")
options = parser.parse_args()
source = options.source
raw = source.read_bytes().replace(b"\r\n", b"\n")
sets = json.loads("\n".join(x for x in raw.decode().splitlines() if not x.startswith("//")))["test_vectors"]
rows = [
    "# STROBEgo published v1.0.2 checkpoints, no round-hash implementation used to generate expected bytes.",
    "# https://raw.githubusercontent.com/mimoo/StrobeGo/master/strobe/test_vectors/test_vectors.json",
    "# Modified format: selected hash operations and checkpoint offsets. Apache-2.0; see STROBEGO-LICENSE.txt.",
    f"# Vendored source SHA256 (LF normalized) {hashlib.sha256(raw).hexdigest()}",
]
for group in sets:
    position = begin = flags = 0
    previous = None
    for index, operation in enumerate(group["operations"]):
        name = operation["name"]
        label = f'{group["name"].replace(" ", "-")}-{index}'
        more = operation["stream"]
        meta = operation["meta"]
        if name == "init":
            assert operation["security"] == 128
            rows.append("\t".join([label, "init", operation["custom_string"].encode().hex(), operation["state_after"]]))
            # metaAD adds its two-byte operation prefix without a permutation.
            position = len(operation["custom_string"].encode()) + 2
            begin, flags = 1, 18
        elif name == "KEY":
            if not more:
                position = begin = 0  # KEY forces a permutation before overwrite.
                flags = 6 + (16 if meta else 0)
            position = (position + len(operation["input_data"]) // 2) % 166
        elif name in ("AD", "PRF") and previous is not None:
            operation_flags = (2 if name == "AD" else 7) + (16 if meta else 0)
            assert not (name == "PRF" and meta)
            rows.append("\t".join([label, "metaAD" if name == "AD" and meta else name,
                                  previous, str(position), str(begin), str(flags), "1" if more else "0",
                                  operation.get("input_data", operation.get("output", "")), operation["state_after"]]))
            if not more:
                begin, flags = position + 1, operation_flags
                position += 2
                if position >= 166:
                    position %= 166
                    begin = 0
                if name == "PRF" and position != 0:
                    position = begin = 0
            else:
                assert flags == operation_flags
            size = len(operation["input_data"]) // 2 if name == "AD" else operation["input_length"]
            if position + size >= 166:
                begin = 0
            position = (position + size) % 166
        else:
            # Every next tested operation after a transport operation follows
            # a new KEY, which resets offsets. Do not guess unsupported offsets.
            previous = None
            continue
        previous = operation["state_after"]

destination = options.destination
expected = "\n".join(rows) + "\n"
if options.check:
    assert destination.read_text(encoding="utf-8") == expected, "STROBEgo fixture changed."
else:
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(expected, encoding="utf-8", newline="\n")
print(json.dumps({"cases": sum(not x.startswith("#") for x in rows), "source_sha256": hashlib.sha256(raw).hexdigest(),
                  "fixture_sha256": hashlib.sha256(destination.read_bytes()).hexdigest()}))
