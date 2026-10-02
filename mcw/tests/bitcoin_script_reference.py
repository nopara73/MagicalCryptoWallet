"""Independent development-only Script oracle and offline corpus preparation.

Uses Python arbitrary-precision integers, struct and bytes, never the Rust parser
to generate expected results. Core v29.0 format vectors are not execution tests.
No Python code or reference sources are part of the shipping application.
"""
import argparse
import hashlib
import json
import random
import re
import struct
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "mcw/tests/bitcoin_script_fixtures"
SOURCES = ROOT / ".artifacts/bitcoin-script-evidence/sources"


def reference_names():
    values = {}
    for name, expression in re.findall(r"(OP_\w+)\s*=\s*(0x[0-9a-fA-F]+|[0-9]+|OP_\w+)",
                                       (SOURCES / "core-script.h").read_text()):
        values[name] = values[expression] if expression.startswith("OP_") else int(expression, 0)
    names = {}
    for name, text in re.findall(r'case\s+(OP_\w+)\s*:\s*return\s+"([^"]+)"',
                                 (SOURCES / "core-script.cpp").read_text()):
        names[values[name]] = text
    return values, names


def number_encode(value):
    if value == 0:
        return b""
    magnitude = abs(value)
    data = magnitude.to_bytes((magnitude.bit_length() + 7) // 8, "little")
    if data[-1] & 128:
        return data + bytes([128 if value < 0 else 0])
    return data[:-1] + bytes([data[-1] | (128 if value < 0 else 0)])


def number_decode(data, maximum, minimal):
    if len(data) > maximum:
        return "size"
    if minimal and data and data[-1] & 127 == 0 and (len(data) == 1 or data[-2] & 128 == 0):
        return "minimal"
    if not data:
        return "0"
    magnitude = int.from_bytes(data, "little") & ~(128 << (8 * (len(data) - 1)))
    value = -magnitude if data[-1] & 128 else magnitude
    return str(value) if -(1 << 63) <= value < (1 << 63) else "overflow"


def min_opcode(data):
    if not data:
        return 0
    if len(data) == 1 and 1 <= data[0] <= 16:
        return 80 + data[0]
    if data == b"\x81":
        return 79
    return len(data) if len(data) <= 75 else 76 if len(data) <= 255 else 77 if len(data) <= 65535 else 78


def push(data, minimal=False):
    op = min_opcode(data)
    if minimal and op in [0, 79, *range(81, 97)]:
        return bytes([op])
    size = len(data)
    prefix = bytes([size]) if size <= 75 else bytes([76, size]) if size <= 255 else (
        b"\x4d" + struct.pack("<H", size) if size <= 65535 else b"\x4e" + struct.pack("<I", size))
    return prefix + data


def core_parse(text, names):
    mapping = {}
    for op, name in names.items():
        if op == 80 or 97 <= op <= 185:
            mapping[name] = op
            mapping[name.removeprefix("OP_")] = op
    result = bytearray()
    for word in re.split(r"[ \t\n]+", text):
        if not word:
            continue
        if re.fullmatch(r"-?[0-9]+", word):
            value = int(word)
            if abs(value) > 0xffffffff:
                raise ValueError("Core decimal range")
            result += push(number_encode(value), True)
        elif word.startswith("0x") and len(word) > 2:
            result += bytes.fromhex(word[2:])
        elif len(word) >= 2 and word.startswith("'") and word.endswith("'"):
            result += push(word[1:-1].encode("ascii"))
        else:
            result.append(mapping[word])
    return bytes(result)


def parse(data):
    """Independent slicing parser. Returns tokens and a typed failure summary."""
    tokens = []
    pos = 0
    while pos < len(data):
        start = pos
        op = data[pos]
        pos += 1
        payload = None
        if op <= 78:
            width = {76: 1, 77: 2, 78: 4}.get(op, 0)
            if len(data) - pos < width:
                return tokens, f"length:{start}:{width}:{len(data)-pos}"
            size = int.from_bytes(data[pos:pos + width], "little") if width else op
            pos += width
            if len(data) - pos < size:
                return tokens, f"push:{start}:{size}:{len(data)-pos}"
            payload = data[pos:pos + size]
            pos += size
        tokens.append((start, op, payload, data[start:pos]))
    return tokens, None


def parse_summary(data):
    tokens, failure = parse(data)
    if failure:
        return "ERR:" + failure
    nonminimal = sum(payload is not None and op != min_opcode(payload) for _, op, payload, _ in tokens)
    maximum = max((len(payload) for _, _, payload, _ in tokens if payload is not None), default=0)
    push_only = int(all(op <= 96 for _, op, _, _ in tokens))
    return f"OK:{len(tokens)}:{nonminimal}:{maximum}:{push_only}"


def core_display(data, names):
    tokens, failure = parse(data)
    words = [number_decode(payload, 4, False) if payload is not None and len(payload) <= 4 else
             payload.hex() if payload is not None else names.get(op, "OP_UNKNOWN")
             for _, op, payload, _ in tokens]
    if failure:
        words.append("[error]")
    return " ".join(words)


def core_format(data, names):
    tokens, failure = parse(data)
    words = []
    end = 0
    for start, op, payload, raw in tokens:
        end = start + len(raw)
        if op == 0:
            words.append("0")
        elif op == 79 or 81 <= op <= 96:
            words.append(str(op - 80))
        elif 97 <= op <= 185:
            words.append(names[op].removeprefix("OP_"))
        elif payload:
            words.extend(["0x" + raw[:-len(payload)].hex(), "0x" + payload.hex()])
        else:
            words.append("0x" + raw.hex())
    if failure:
        words.append("0x" + data[end:].hex())
    return " ".join(words)


def prepare():
    FIXTURES.mkdir(exist_ok=True)
    _, names = reference_names()
    vectors = json.loads((SOURCES / "core-script-tests.json").read_text())
    rows = []
    for index, row in enumerate(vectors):
        if len(row) < 4:
            continue
        scripts = row[1:3] if isinstance(row[0], list) else row[:2]
        for position, text in enumerate(scripts):
            data = core_parse(text, names)
            rows.append("\t".join([f"core-{index}-{position}", text.encode().hex(), data.hex(),
                                   parse_summary(data), core_display(data, names).encode().hex(),
                                   core_format(data, names).encode().hex(), "."]))
    (FIXTURES / "core_vectors.tsv").write_text("# id\tcore_input_utf8_hex\tscript_hex\tparse_summary\tcore_asm_utf8_hex\tformat_utf8_hex\tend\n" +
                                              "\n".join(rows) + "\n", encoding="utf-8", newline="\n")
    values = [0, 1, -2, 127, 128, -255, 256, (1 << 15) - 1, -(1 << 16),
              (1 << 24) - 1, 1 << 31, 1 - (1 << 32), 1 << 40]
    offsets = [1, 0x79, 0x80, 0x81, 0xff, 0x7fff, 0x8000, 0xffff, 0x10000]
    numbers = sorted({v + sign * o for v in values for o in offsets for sign in [-1, 0, 1]} |
                     {-(1 << 63), (1 << 63) - 1})
    (FIXTURES / "numbers.tsv").write_text("# decimal\tminimal_bytes_hex\tend\n" + "\n".join(
        f"{v}\t{number_encode(v).hex()}\t." for v in numbers) + "\n", encoding="utf-8", newline="\n")
    hashes = {path.name: {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "bytes": path.stat().st_size}
              for path in sorted(SOURCES.iterdir()) if path.is_file()}
    hashes.update({path.name: {"sha256": hashlib.sha256(path.read_bytes()).hexdigest(), "bytes": path.stat().st_size}
                   for path in [FIXTURES / "core_vectors.tsv", FIXTURES / "numbers.tsv"]})
    manifest = {"core_version": "v29.0", "wallet_reference_version": "NBitcoin v10.0.13",
                "core_format_vectors": len(rows), "number_vectors": len(numbers), "hashes": hashes,
                "scope": "format only; upstream execution result/flags/witness ignored"}
    (FIXTURES / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8", newline="\n")
    (FIXTURES / "BITCOIN_CORE_LICENSE.txt").write_bytes((SOURCES / "core-COPYING").read_bytes())
    (FIXTURES / "NBITCOIN_LICENSE.txt").write_bytes((SOURCES / "nbitcoin-LICENSE").read_bytes())
    print(json.dumps({"core_format_vectors": len(rows), "number_vectors": len(numbers)}))


def differential(binary, output):
    rng = random.Random(0x0d00b17c)
    _, names = reference_names()
    requests = []
    expected = []
    for length in [0, 1, 2, 3, 4, 5, 20, 32, 75, 76, 255, 256, 520, 65535, 65536]:
        data = rng.randbytes(length)
        for op, prefix in [(None, push(data)), (76, bytes([76, length]) + data if length <= 255 else None),
                           (77, b"\x4d" + length.to_bytes(2, "little") + data if length <= 65535 else None),
                           (78, b"\x4e" + length.to_bytes(4, "little") + data)]:
            if prefix is not None:
                requests.append("P\t" + prefix.hex())
                expected.append(parse_summary(prefix))
    for _ in range(6000):
        data = rng.randbytes(rng.randrange(0, 300))
        if rng.randrange(2):
            data = push(data, bool(rng.randrange(2))) + bytes([rng.randrange(79, 256)])
        for command, oracle in [("P", parse_summary(data)), ("F", core_format(data, names).encode().hex()),
                                ("D", core_display(data, names).encode().hex())]:
            requests.append(command + "\t" + data.hex())
            expected.append(oracle)
    # Exhaustive one-byte and two-byte number forms; arbitrary sign/padding lengths.
    numeric_data = [bytes([x]) for x in range(256)] + [x.to_bytes(2, "little") for x in range(65536)]
    numeric_data += [rng.randbytes(rng.randrange(0, 10)) for _ in range(2500)]
    for i, data in enumerate(numeric_data):
        maximum = [4, 5, 9][i % 3]
        minimal = bool(i % 2)
        requests.append(f"N\t{data.hex()}\t{maximum}\t{int(minimal)}")
        expected.append(number_decode(data, maximum, minimal))
    values = [-(1 << 63), (1 << 63) - 1] + [rng.randrange(-(1 << 63), 1 << 63) for _ in range(3000)]
    for value in values:
        requests.append(f"E\t{value}")
        expected.append(number_encode(value).hex())
    result = subprocess.run([str(binary)], input="\n".join(requests) + "\n", text=True,
                            encoding="utf-8", capture_output=True, check=True)
    actual = result.stdout.splitlines()
    if len(actual) != len(expected):
        raise AssertionError(f"probe response count {len(actual)} != {len(expected)}; stderr={result.stderr[:300]}")
    for i, (got, want) in enumerate(zip(actual, expected)):
        if got != want:
            raise AssertionError(f"differential mismatch {i}: request={requests[i][:150]!r} actual={got!r} expected={want!r}")
    evidence = {"seed": "0x0d00b17c", "checks": len(expected), "parser_display_format_scripts": 6000,
                "number_decode_cases": len(numeric_data), "number_encode_cases": len(values),
                "probe_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "oracle_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "production_release": False}
    output.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(evidence))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--probe", type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / ".artifacts/bitcoin-script-evidence/differential.json")
    args = parser.parse_args()
    if args.prepare:
        prepare()
    if args.probe:
        differential(args.probe, args.output)
