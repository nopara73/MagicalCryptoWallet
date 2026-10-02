"""Extract the official BIP174/BIP370 vectors, using Python's independent codecs.

Test tooling only: no Python code or downloaded BIP source ships in mcw.
Usage: python mcw/tests/psbt_vectors.py SOURCE_DIRECTORY OUTPUT_TSV
The source snapshot used here is bitcoin/bips commit
3a10b5b5f0a7586df8928d580a3009744ebb2079 (both BIPs are BSD-2-Clause).
"""

import base64
import hashlib
import pathlib
import re
import sys


def extract(path):
    state = None
    locktime = "-"
    case = None
    hex_bytes = None
    encoded = None
    rows = []

    def finish():
        if case is None or hex_bytes is None:
            return
        raw = bytes.fromhex(hex_bytes)
        reference = base64.b64encode(raw).decode("ascii")
        if encoded is not None:
            assert base64.b64decode(encoded, validate=True) == raw, case
            assert reference == encoded, case
        assert state in ("invalid", "valid", "signer"), (path, case, state)
        rows.append((path.stem, state, locktime, case, raw.hex(), reference))

    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if "following are invalid PSBTs" in line:
            finish()
            case = hex_bytes = encoded = None
            state, locktime = "invalid", "-"
        elif "following are valid PSBTs" in line:
            finish()
            case = hex_bytes = encoded = None
            state, locktime = "valid", "-"
        elif line == "Fails Signer checks":
            finish()
            case = hex_bytes = encoded = None
            state, locktime = "signer", "-"
        elif line.startswith("The timelock for the following PSBTs"):
            finish()
            case = hex_bytes = encoded = None
            state = "valid"
            match = re.search(r"computed to be (\d+)", line)
            locktime = match.group(1) if match else "incompatible"
        elif line.startswith("=="):
            finish()
            case = hex_bytes = encoded = None
        elif line.startswith("* Case:"):
            finish()
            case = line.removeprefix("* Case:").strip()
            hex_bytes = encoded = None
        elif line.startswith("* Bytes in Hex:"):
            finish()
            raw_hex = re.search(r"<pre>(.*?)</pre>", line).group(1)
            case = f"Role example at source line {line_number}" if raw_hex.startswith("70736274ff") else None
            hex_bytes, encoded = raw_hex, None
            state, locktime = "valid", "-"
        elif case is not None and line.startswith("** Bytes in Hex:"):
            if hex_bytes is not None:
                finish()
                case = f"Additional vector at source line {line_number}"
                encoded = None
            hex_bytes = re.search(r"<pre>(.*?)</pre>", line).group(1)
        elif case is not None and encoded is None and line.startswith("** Base64 String:"):
            encoded = re.search(r"<pre>(.*?)</pre>", line).group(1)
    finish()
    return rows


if __name__ == "__main__":
    source, output = map(pathlib.Path, sys.argv[1:])
    paths = [source / "bip-0174.mediawiki", source / "bip-0370.mediawiki"]
    rows = [row for path in paths for row in extract(path)]
    with output.open("w", encoding="utf-8", newline="\n") as handle:
        handle.write("# BIP174/BIP370 official vectors, BSD-2-Clause.\n")
        handle.write("# Author: Ava Chow (BIP174 and BIP370).\n")
        handle.write("# bitcoin/bips commit 3a10b5b5f0a7586df8928d580a3009744ebb2079\n")
        for path in paths:
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            handle.write(f"# {path.name} SHA256 {digest}\n")
        handle.write("# source\tresult\tlocktime\tcase\thex\tbase64\n")
        for row in rows:
            handle.write("\t".join(row) + "\n")
    for path in paths:
        selected = [row for row in rows if row[0] == path.stem]
        for state in ("invalid", "valid", "signer"):
            print(path.stem, state, sum(row[1] == state for row in selected))
    print("All published hex/Base64 pairs matched Python's independent codecs.")
