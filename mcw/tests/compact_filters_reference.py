"""Independent development-only BIP158/SipHash fixture generator (stdlib only).

No part of this script is shipped or called by mcw. Official input bytes are
verified before generating Rust constants. Uses bit strings / arbitrary-precision
integers instead of the production streaming bit codec and fixed-width arithmetic.
"""

import argparse
import hashlib
import json
from pathlib import Path
import random
import re

MASK = (1 << 64) - 1


def rotate(x, n):
    return ((x << n) | (x >> (64 - n))) & MASK


def round_state(state):
    a, b, c, d = state
    a = (a + b) & MASK
    b = rotate(b, 13) ^ a
    c = (c + d) & MASK
    d = rotate(d, 16) ^ c
    a = (rotate(a, 32) + d) & MASK
    d = rotate(d, 21) ^ a
    c = (c + b) & MASK
    b = rotate(b, 17) ^ c
    return a, b, rotate(c, 32), d


def siphash(key, message):
    k0 = int.from_bytes(key[:8], "little")
    k1 = int.from_bytes(key[8:], "little")
    state = (0x736F6D6570736575 ^ k0, 0x646F72616E646F6D ^ k1,
             0x6C7967656E657261 ^ k0, 0x7465646279746573 ^ k1)
    full = len(message) // 8 * 8
    words = [int.from_bytes(message[i:i + 8], "little") for i in range(0, full, 8)]
    words.append(int.from_bytes(message[full:], "little") | ((len(message) & 255) << 56))
    for word in words:
        a, b, c, d = state
        state = round_state(round_state((a, b, c, d ^ word)))
        a, b, c, d = state
        state = a ^ word, b, c, d
    a, b, c, d = state
    state = a, b, c ^ 255, d
    for _ in range(4):
        state = round_state(state)
    a, b, c, d = state
    return a ^ b ^ c ^ d


def compact_size(n):
    if n < 253:
        return bytes([n])
    if n <= 65535:
        return b"\xfd" + n.to_bytes(2, "little")
    if n <= 0xFFFFFFFF:
        return b"\xfe" + n.to_bytes(4, "little")
    raise ValueError("BIP158 N must be <2^32")


def construct(key, items, p, m):
    items = set(items)
    count = len(items)
    values = sorted((siphash(key, item) * (count * m)) >> 64 for item in items)
    codes = []
    previous = 0
    for value in values:
        delta = value - previous
        quotient = delta >> p
        remainder = delta & ((1 << p) - 1)
        codes.append("1" * quotient + "0" + (format(remainder, f"0{p}b") if p else ""))
        previous = value
    bits = "".join(codes)
    bits += "0" * ((-len(bits)) % 8)
    body = int(bits, 2).to_bytes(len(bits) // 8, "big") if bits else b""
    return compact_size(count) + body, values


def sha256d(data):
    return hashlib.sha256(hashlib.sha256(data).digest()).digest()


class BlockReader:
    def __init__(self, data):
        self.data = data
        self.position = 0

    def take(self, n):
        result = self.data[self.position:self.position + n]
        if len(result) != n:
            raise ValueError("truncated reference block")
        self.position += n
        return result

    def count(self):
        first = self.take(1)[0]
        if first < 253:
            return first
        width = {253: 2, 254: 4, 255: 8}[first]
        n = int.from_bytes(self.take(width), "little")
        assert n >= {253: 253, 254: 65536, 255: 0x100000000}[first]
        return n

    def blob(self):
        return self.take(self.count())


def block_outputs(block):
    reader = BlockReader(block)
    reader.take(80)
    scripts = []
    previous_count = 0
    for tx_index in range(reader.count()):
        reader.take(4)
        witness = reader.data[reader.position:reader.position + 2] == b"\0\1"
        if witness:
            reader.take(2)
        input_count = reader.count()
        for _ in range(input_count):
            reader.take(36)
            reader.blob()
            reader.take(4)
        if tx_index != 0:
            previous_count += input_count
        for _ in range(reader.count()):
            reader.take(8)
            scripts.append(reader.blob())
        if witness:
            for _ in range(input_count):
                for _ in range(reader.count()):
                    reader.blob()
        reader.take(4)
    assert reader.position == len(block)
    return scripts, previous_count


def text_array(items):
    return "&[" + ", ".join('"' + x.hex() + '"' for x in items) + "]"


def generate(reference_json, sip_vectors, output):
    raw_json = reference_json.read_bytes()
    raw_sip = sip_vectors.read_bytes()
    assert hashlib.sha256(raw_json).hexdigest() == "d9049756f744e561b882a8eff507582fb7cd74ed9cf5542bdac58257449ee2a2"
    assert hashlib.sha256(raw_sip).hexdigest() == "212c44114a63c6d84710b8627f3bc5ce155698accf2ea7bbff1c4b69c9f48d31"
    source = raw_sip.decode().split("const uint8_t vectors_sip64[64][8] = {", 1)[1].split("};", 1)[0]
    vector_bytes = bytes(int(h, 16) for h in re.findall(r"0x([0-9a-fA-F]{2})", source))
    assert len(vector_bytes) == 512
    known = [int.from_bytes(vector_bytes[n:n + 8], "little") for n in range(0, 512, 8)]
    for n, expected in enumerate(known):
        assert siphash(bytes(range(16)), bytes(range(n))) == expected

    lines = [
        "// Generated by compact_filters_reference.py; data only, no runtime dependencies.",
        "// BIP158 / Bitcoin Core official vectors are byte-identical (CC0 BIP source).",
        "// testnet-19.json SHA256: " + hashlib.sha256(raw_json).hexdigest(),
        "// veorq/SipHash vectors.h SHA256: " + hashlib.sha256(raw_sip).hexdigest(),
        "const SIPHASH_VECTORS: [u64; 64] = [",
        *[f"    0x{n:016x}," for n in known],
        "];",
        "const LONG_SIPHASH_VECTORS: &[(usize, u64)] = &[",
        *[f"    ({n}, 0x{siphash(bytes(range(16)), bytes(i & 255 for i in range(n))):016x}),"
          for n in [64, 65, 127, 128, 255, 256, 257, 511, 512, 1024, 4096]],
        "];",
        "const OFFICIAL_VECTORS: &[OfficialVector] = &[",
    ]
    official_count = 0
    for row in json.loads(raw_json):
        if len(row) < 7:
            continue
        height, hash_text, block_hex, previous_scripts, previous_header, encoded, header = row[:7]
        block = bytes.fromhex(block_hex)
        block_hash = bytes.fromhex(hash_text)[::-1]
        assert sha256d(block[:80]) == block_hash
        outputs, previous_count = block_outputs(block)
        assert previous_count == len(previous_scripts)
        spent = [bytes.fromhex(x) for x in previous_scripts]
        elements = [x for x in outputs if x and x[0] != 106] + [x for x in spent if x]
        built, _ = construct(block_hash[:16], elements, 19, 784931)
        assert built.hex() == encoded, (height, built.hex(), encoded)
        filter_hash = sha256d(built)
        assert sha256d(filter_hash + bytes.fromhex(previous_header)[::-1])[::-1].hex() == header
        lines += [
            "    OfficialVector {",
            f"        height: {height},",
            f'        block_hash: "{hash_text}",',
            f"        outputs: {text_array(outputs)},",
            f"        spent: {text_array(spent)},",
            f'        previous_header: "{previous_header}",',
            f'        encoded: "{encoded}",',
            f'        filter_hash_raw: "{filter_hash.hex()}",',
            f'        header: "{header}",',
            "    },",
        ]
        official_count += 1
    lines += ["];", "const REFERENCE_CASES: &[ReferenceCase] = &["]
    rng = random.Random(0xB1580700)
    parameters = [(0, 1), (0, 17), (1, 31), (2, 42), (5, 79),
                  (19, 784931), (20, 1 << 20), (31, 0xFFFFFFFF), (63, 0xFFFFFFFF)]
    for case in range(90):
        p, m = parameters[case % len(parameters)]
        key = rng.randbytes(16)
        items = [rng.randbytes(rng.randrange(0, 40)) for _ in range(case % 37)]
        if case % 11 == 0:
            items.append(rng.randbytes(1024))
        if items:
            items += items[:3]
        queries = [rng.randbytes(rng.randrange(0, 30)) for _ in range(6)] + items[::3]
        rng.shuffle(queries)
        encoded, values = construct(key, items, p, m)
        results = [bool(values) and (siphash(key, x) * (len(set(items)) * m)) >> 64 in values for x in queries]
        lines += [
            "    ReferenceCase {",
            f'        key: "{key.hex()}", p: {p}, m: {m},',
            f"        items: {text_array(items)},",
            f'        encoded: "{encoded.hex()}",',
            "        values: &[" + ", ".join(str(n) for n in values) + "],",
            f"        queries: {text_array(queries)},",
            "        results: &[" + ", ".join(str(x).lower() for x in results) + "],",
            "    },",
        ]
    lines += ["];"]
    output.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"Verified {official_count} official BIP158 filters/headers, 64 upstream SipHash vectors; generated 90 independent GCS cases.")
    print("Fixture SHA256: " + hashlib.sha256(output.read_bytes()).hexdigest())


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--bip158", type=Path, required=True)
    parser.add_argument("--siphash", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    generate(args.bip158, args.siphash, args.output)
