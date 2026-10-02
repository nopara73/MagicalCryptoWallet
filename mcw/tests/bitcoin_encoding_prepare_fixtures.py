"""Refresh offline test vectors from separately downloaded primary sources.

Test tooling only; it is never part of mcw or required to run the wallet.
Download locations, retrieval hashes and licenses are recorded in SOURCES.md.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path


def extract_bip(path, bip):
    source = path.read_text(encoding="utf-8")
    begin = source.index("===Test vectors===" if bip == 173 else "==Test vectors==")
    end = source.index("===Checksum design===" if bip == 173 else "==Appendix:", begin)
    category = None
    rows = []
    for line in source[begin:end].splitlines():
        if "strings are valid Bech32" in line:
            category = "generic-valid"
        elif "not valid Bech32" in line:
            category = "generic-invalid"
        elif "following list gives valid segwit" in line:
            category = "witness-valid"
        elif "following list gives invalid segwit" in line:
            category = "witness-invalid"
        if not line.startswith("* ") or category is None:
            continue
        values = re.findall(r"<tt>(.*?)</tt>", line)
        if not values:
            continue
        text = values[0]
        prefix = re.match(r"\* 0x([0-9A-Fa-f]{2}) \+", line)
        if prefix:
            text = chr(int(prefix[1], 16)) + text
        suffix = re.search(r"</tt> \+ 0x([0-9A-Fa-f]{2})", line)
        if suffix:
            text += chr(int(suffix[1], 16))
        script = values[1] if category == "witness-valid" else "-"
        # BIP350 supersedes the BIP173 v1+ checksum. Retain those original
        # vectors and explicitly test their rejection by the current codec.
        expected = category
        if bip == 173 and category == "witness-valid" and not script.startswith("00"):
            expected = "witness-obsolete"
        rows.append(f"{bip}\t{expected}\t{text.encode('utf-8').hex()}\t{script}")
    return rows


def main():
    args = argparse.ArgumentParser()
    args.add_argument("reference_directory", type=Path)
    args.add_argument("--output", type=Path, default=Path(__file__).parent / "bitcoin_encoding_fixtures")
    options = args.parse_args()
    ref, out = options.reference_directory, options.output
    out.mkdir(parents=True, exist_ok=True)
    rows = extract_bip(ref / "bip-0173.mediawiki", 173) + extract_bip(ref / "bip-0350.mediawiki", 350)
    (out / "bip_vectors.tsv").write_text(
        "# bip\tcategory\tinput_utf8_hex\texpected_script_hex_or_dash\n" + "\n".join(rows) + "\n",
        encoding="utf-8",
    )
    raw_base58 = json.loads((ref / "base58_encode_decode.json").read_text(encoding="utf-8"))
    (out / "base58_vectors.tsv").write_text(
        "# input_hex\tbase58_text; dash denotes empty input/output\n" + "\n".join(f"{row[0] or '-'}\t{row[1] or '-'}" for row in raw_base58) + "\n",
        encoding="utf-8",
    )
    keys = json.loads((ref / "key_io_valid.json").read_text(encoding="utf-8"))
    address_rows = [f"{meta['chain']}\t{text}\t{script}" for text, script, meta in keys if not meta["isPrivkey"]]
    (out / "core_addresses.tsv").write_text(
        "# chain\taddress\texpected_script_hex\n" + "\n".join(address_rows) + "\n", encoding="utf-8"
    )
    invalid = json.loads((ref / "key_io_invalid.json").read_text(encoding="utf-8"))
    (out / "core_invalid_addresses.tsv").write_text(
        "# invalid_input_utf8_hex\n" + "\n".join(row[0].encode("utf-8").hex() for row in invalid) + "\n",
        encoding="utf-8",
    )
    for name in ["bip-0173.mediawiki", "bip-0350.mediawiki", "base58_encode_decode.json",
                 "key_io_valid.json", "key_io_invalid.json", "segwit_addr.py"]:
        path = ref / name
        print(path.name, hashlib.sha256(path.read_bytes()).hexdigest())
    print(f"BIP rows={len(rows)}, Base58 rows={len(raw_base58)}, Core addresses={len(address_rows)}, Core invalid={len(invalid)}")


if __name__ == "__main__":
    main()
