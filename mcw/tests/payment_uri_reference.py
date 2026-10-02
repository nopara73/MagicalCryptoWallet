"""Independent Decimal/urllib oracle for the temporary Rust test harness.

Test tooling only; never shipped, imported, or called by the mcw application.
Run through payment_uri_verify.ps1 or pass --executable explicitly. No downloads
or third-party Python packages are used. All test input is synthetic/public.
"""
import argparse
from decimal import Decimal, localcontext
import json
from pathlib import Path
import random
import re
import subprocess
from urllib.parse import quote, unquote_to_bytes


MAX_SATOSHIS = 2_100_000_000_000_000
MAX_COMPONENT_BYTES = 4_000
SEED = 0xB1_21_04


def decimal_amount(text):
    if not re.fullmatch(r"(?:[0-9]+(?:\.[0-9]{0,8})?|\.[0-9]{1,8})", text):
        return "ERR"
    with localcontext() as context:
        context.prec = max(100, len(text) + 20)
        satoshis = Decimal(text) * 100_000_000
    if satoshis != satoshis.to_integral_value() or not 0 <= satoshis <= MAX_SATOSHIS:
        return "ERR"
    return str(int(satoshis))


def btc_text(satoshis):
    if not 0 <= satoshis <= MAX_SATOSHIS:
        return "ERR"
    return format(Decimal(satoshis) / 100_000_000, ".8f").rstrip("0").rstrip(".")


def decode(text, form):
    if not re.fullmatch(r"(?:[^%]|%[0-9a-fA-F]{2})*", text):
        return "ERR"
    if form:
        text = text.replace("+", " ")
    try:
        data = unquote_to_bytes(text)
        if len(data) > MAX_COMPONENT_BYTES:
            return "ERR"
        return data.decode("utf-8", errors="strict").encode("utf-8").hex()
    except UnicodeError:
        return "ERR"


def cases():
    rng = random.Random(SEED)
    for text in ["0", ".1", "1.", "0001.23", "20.3", "50.00", "21000000",
                 "20999999.99999999", ".00000001", "21000000.00000001", "",
                 ".", "-0", "-1", "+1", "1e-8", "1,000", "100'000", "1..0",
                 "0.000000001", "1.000000000", "NaN", " 1", "1 ", "١", "１"]:
        yield "A", text, decimal_amount(text)
    for _ in range(5_000):
        satoshis = rng.randrange(MAX_SATOSHIS + 1)
        text = format(Decimal(satoshis) / 100_000_000, ".8f")
        if rng.choice([False, True]):
            text = "0" * rng.randrange(5) + text
        yield "A", text, decimal_amount(text)
        yield "S", str(satoshis), btc_text(satoshis)
    for satoshis in [0, 1, 10, 100, 100_000_000, MAX_SATOSHIS - 1,
                     MAX_SATOSHIS, MAX_SATOSHIS + 1, 2**64 - 1]:
        yield "S", str(satoshis), btc_text(satoshis)
    for _ in range(1_000):
        text = f"{rng.randrange(21_000_001)}.{rng.randrange(10**9):09d}"
        yield "A", text, "ERR"
    texts = ["", "abc-._~", " &+=?#%/", "Árvíztűrő tükörfúrógép", "東京 🦀",
             "\0\r\n\t", "100%", "+", "a=b&c=d", "e\u0301", "\ufffd"]
    for _ in range(1_000):
        codepoints = [rng.randrange(0x110000) for _ in range(rng.randrange(1, 20))]
        texts.append("".join(chr(cp) for cp in codepoints if not 0xD800 <= cp <= 0xDFFF))
    for text in texts:
        encoded = quote(text, safe="-._~", encoding="utf-8", errors="strict")
        yield "E", text, encoded.encode().hex()
        yield "D", encoded, decode(encoded, False)
        yield "F", encoded, decode(encoded, True)
    for text in ["%", "%0", "%GG", "%u0041", "%+1", "%80", "%FF", "%C0%AF",
                 "%ED%A0%80", "%F4%90%80%80", "%E2%82", "%2541", "%2b+",
                 "A+B", "a%3Db%26c", "raw東京", "x" * (MAX_COMPONENT_BYTES + 1)]:
        yield "D", text, decode(text, False)
        yield "F", text, decode(text, True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    entries = list(cases())
    wire = "".join(f"{kind}\t{text.encode('utf-8').hex()}\n" for kind, text, _ in entries)
    result = subprocess.run([str(args.executable.resolve())], input=wire, text=True,
                            capture_output=True, check=True, timeout=60)
    outputs = result.stdout.splitlines()
    if len(outputs) != len(entries):
        raise AssertionError(f"Expected {len(entries)} replies, got {len(outputs)}")
    for index, ((kind, text, expected), actual) in enumerate(zip(entries, outputs)):
        if actual != expected:
            raise AssertionError(f"Case {index} {kind} {text!r}: {actual!r} != {expected!r}")
    counts = {kind: sum(entry[0] == kind for entry in entries) for kind in ["A", "S", "E", "D", "F"]}
    evidence = {"oracle": "Python stdlib Decimal and urllib.parse; strict UTF-8",
                "seed": SEED, "cases": len(entries), "counts": counts, "passed": True}
    if args.evidence:
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        args.evidence.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(evidence, sort_keys=True))


if __name__ == "__main__":
    main()
