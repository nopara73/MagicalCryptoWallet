"""Independent Decimal/urllib oracle for the temporary Rust test harness.

Test tooling only; never shipped, imported, or called by the mcw application.
Run through payment_uri_verify.ps1 or pass --executable explicitly. No downloads
or third-party Python packages are used. All test input is synthetic/public.
"""
import argparse
from decimal import Decimal, localcontext
import hashlib
import json
from pathlib import Path
import random
import re
import subprocess
import struct
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


def service_cases():
    """Independent Python struct/Decimal/urllib payload transcripts.

    Address descriptors use a published BIP350 program or independent integer
    Base58Check extraction with stdlib SHA256; production uses the actual peer
    service. No test-only descriptor/validator can enter application code.
    """
    address = "18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX"
    witness = "bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0"
    witness_program = bytes.fromhex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

    def field(value):
        data = value.encode("utf-8") if isinstance(value, str) else value
        return struct.pack("<I", len(data)) + data

    def base58_program(value):
        number = 0
        for char in value:
            number = number * 58 + alphabet.index(char)
        decoded = b"\0" * (len(value) - len(value.lstrip("1"))) + number.to_bytes((number.bit_length() + 7) // 8, "big")
        assert hashlib.sha256(hashlib.sha256(decoded[:-4]).digest()).digest()[:4] == decoded[-4:]
        return decoded[1:-4]

    def packet(operation, request, reply=None, error=None):
        return "H", struct.pack("<H", operation) + request, f"ERR:{error}" if error else "OK:" + reply.hex()

    for btc in ["0", ".1", "1.", "20.3", "20999999.99999999", "21000000", "",
                "-1", "1e8", "1,000", "0.000000001", "21000000.00000001"]:
        expected = decimal_amount(btc)
        yield packet(0x0402, b"\1" + field(btc),
                     b"\1" + struct.pack("<Q", int(expected)) if expected != "ERR" else None,
                     7 if not btc else 8 if expected == "ERR" else None)
    for satoshis in [0, 1, 100, 100_000_000, MAX_SATOSHIS, MAX_SATOSHIS + 1, 2**64 - 1]:
        expected = btc_text(satoshis)
        yield packet(0x0403, b"\1" + struct.pack("<Q", satoshis),
                     b"\1" + field(expected) if expected != "ERR" else None,
                     8 if expected == "ERR" else None)
    for value in ["", "A+ &=%?", "東京 🦀", "\0\r\n", " " * MAX_COMPONENT_BYTES]:
        expected = quote(value, safe="-._~", encoding="utf-8")
        yield packet(0x0404, b"\1" + field(value), b"\1" + field(expected))
    for form in [False, True]:
        for value in ["", "A+%2bB", "%2541", "%C3%81%E6%9D%B1", "%00", "%FF", "%GG"]:
            expected = decode(value, form)
            yield packet(0x0405, bytes([1, int(form)]) + field(value),
                         b"\1" + field(bytes.fromhex(expected)) if expected != "ERR" else None,
                         5 if expected == "ERR" else None)

    def parsed_reply(text, selected, uri_request=False, amount=None, label=None, message=None, optional=()):
        original = text.strip()
        payload_address = original.split(":", 1)[1].split("?", 1)[0] if uri_request else original
        is_witness = payload_address.lower() == witness
        data = witness_program if is_witness else base58_program(payload_address)
        canonical = witness if is_witness else payload_address
        kind = 2 if is_witness else 1 if payload_address[0] in "23" else 0
        reply = bytes([1, int(uri_request), selected, kind, 1 if is_witness else 255])
        reply += field(data) + field(payload_address) + field(canonical)
        flags = int(amount is not None) | (int(label is not None) << 1) | (int(message is not None) << 2)
        reply += bytes([flags])
        if amount is not None:
            reply += struct.pack("<Q", amount)
        if label is not None:
            reply += field(label)
        if message is not None:
            reply += field(message)
        reply += bytes([int(uri_request)])
        if uri_request:
            reply += field(original)
        reply += struct.pack("<I", len(optional))
        for name, value in optional:
            reply += field(name) + bytes([int(value is not None)])
            if value is not None:
                reply += field(value)
        return reply

    for text in [address, f"  {address}  ", "3EktnHQD7RiAE6uzMj2ZifT9YgRrkSgzQX", witness, witness.upper()]:
        yield packet(0x0400, bytes([1, 0, 1]) + field(text), parsed_reply(text, 0))
    for selected in [1, 2, 3, 4]:
        for value in ["mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn", "2MzQwSSnBHWHqSAqtTVQ6v47XtaisrJa1Vc"]:
            yield packet(0x0400, bytes([1, selected, 1]) + field(value), parsed_reply(value, selected))
    for form in [False, True]:
        text = f"BITCOIN:{witness.upper()}?amount=0.00000001&label=A+%2bB&message=%2541&pj=opaque&flag&empty="
        reply = parsed_reply(text, 0, True, 1, "A +B" if form else "A++B", "%41",
                             [("pj", "opaque"), ("flag", None), ("empty", "")])
        yield packet(0x0400, bytes([1, 0, int(form)]) + field(text), reply)
    for flags, satoshis, label, message in [(0, None, None, None), (1, 0, None, None),
        (2, None, "東京 &+", None), (7, 1, "東京", "")]:
        request = bytes([1, 0]) + field(address) + bytes([flags])
        params = []
        if satoshis is not None:
            request += struct.pack("<Q", satoshis)
            params.append("amount=" + btc_text(satoshis))
        if label is not None:
            request += field(label)
            params.append("label=" + quote(label, safe="-._~"))
        if message is not None:
            request += field(message)
            params.append("message=" + quote(message, safe="-._~"))
        reply = "bitcoin:" + address + ("?" + "&".join(params) if params else "")
        yield packet(0x0401, request, b"\1" + field(reply))
    for operation in range(0x0400, 0x0406):
        yield packet(operation, b"", error=101)
        yield packet(operation, b"\2", error=102)
        yield packet(operation, b"\0" * 32_001, error=107)
    yield packet(0x0406, b"", error=100)
    yield packet(0x0400, bytes([1, 5, 0]) + field(address), error=103)
    yield packet(0x0400, bytes([1, 0, 2]) + field(address), error=104)
    yield packet(0x0404, b"\1" + field(b"\xff"), error=105)
    yield packet(0x0401, bytes([1, 0]) + field(address) + b"\x08", error=106)
    yield packet(0x0402, b"\1" + field("1") + b"\0", error=101)
    for query, code in [("req-sp=x", 9), ("amount=1&Amount=2", 6), ("amount=", 7),
                        ("amount=0.000000001", 8), ("label=%FF", 5)]:
        yield packet(0x0400, bytes([1, 0, 1]) + field(f"bitcoin:{address}?{query}"), error=code)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    entries = list(cases()) + list(service_cases())
    wire = "".join(f"{kind}\t{(text if isinstance(text, bytes) else text.encode('utf-8')).hex()}\n" for kind, text, _ in entries)
    result = subprocess.run([str(args.executable.resolve())], input=wire, text=True,
                            capture_output=True, check=True, timeout=60)
    outputs = result.stdout.splitlines()
    if len(outputs) != len(entries):
        raise AssertionError(f"Expected {len(entries)} replies, got {len(outputs)}")
    for index, ((kind, text, expected), actual) in enumerate(zip(entries, outputs)):
        if actual != expected:
            raise AssertionError(f"Case {index} {kind} {text!r}: {actual!r} != {expected!r}")
    counts = {kind: sum(entry[0] == kind for entry in entries) for kind in ["A", "S", "E", "D", "F", "H"]}
    evidence = {"oracle": "Python stdlib Decimal and urllib.parse; strict UTF-8",
                "seed": SEED, "cases": len(entries), "counts": counts, "passed": True}
    if args.evidence:
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        args.evidence.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(evidence, sort_keys=True))


if __name__ == "__main__":
    main()
