"""Independent NIP-01 tuple/UTF-8 JSON/hash oracle, using Python stdlib only.

Inputs are synthetic. No production serializer or SHA-256 creates expectations.
NIP-01: https://github.com/nostr-protocol/nips/blob/master/01.md
"""
import argparse
import hashlib
import json
import pathlib
import random
import struct
import subprocess

KEY = bytes.fromhex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
MAX_REQUEST = 1_048_560


def request(key, timestamp, kind, tags, content):
    def text(value):
        data = value.encode("utf-8")
        return struct.pack("<I", len(data)) + data
    result = bytearray(struct.pack("<B32sqiI", 1, key, timestamp, kind, len(tags)))
    for tag in tags:
        result += struct.pack("<I", len(tag))
        for value in tag:
            result += text(value)
    result += text(content)
    return bytes(result)


def expected(key, timestamp, kind, tags, content):
    value = [0, key.hex(), timestamp, kind, tags, content]
    data = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    prefix = "HASH" if len(data) > 1_048_576 else data.hex()
    return prefix + "\t" + hashlib.sha256(data).hexdigest()


def vectors():
    yield KEY, 0, 1, [], ""
    yield KEY, 1700000000, 1, [["version", "2.5.0"], ["unicode", "\u00e9", "e\u0301", "\U0001f9d9", "\u2028", "\u2029"]], "quote\" slash/ backslash\\ controls\b\t\n\f\r\0 <tag> apostrophe'"
    # Every Unicode scalar, including noncharacters, in bounded disjoint pages.
    page = []
    for value in range(0x110000):
        if 0xD800 <= value <= 0xDFFF:
            continue
        page.append(chr(value))
        if len(page) == 4096:
            yield KEY, 253402300799, 1, [["all-scalars", page[0], page[-1]]], "".join(page)
            page = []
    if page:
        yield KEY, -62135596800, 1, [], "".join(page)
    for timestamp in [-(1 << 63), -62135596800, -1, 0, 1, (1 << 53) - 1, 1 << 53, 253402300799, (1 << 63) - 1]:
        for kind in [-(1 << 31), -1, 0, 1, 65535, (1 << 31) - 1]:
            yield KEY, timestamp, kind, [[], ["", ""], ["p", "same"], ["p", "same"]], ""
    rng = random.Random(0x4E49503031)
    alphabet = "abc/<>\"'\\\0\b\t\n\f\r\u001f\u007f\u00e9e\u0301\u2028\u2029\U0001f9d9\U0010ffff"
    for _ in range(1024):
        text = lambda: "".join(rng.choice(alphabet) for _ in range(rng.randrange(50)))
        tags = [[text() for _ in range(rng.randrange(7))] for _ in range(rng.randrange(9))]
        yield rng.randbytes(32), rng.randrange(-(1 << 63), 1 << 63), rng.randrange(-(1 << 31), 1 << 31), tags, text()
    yield KEY, 0, 1, [], "\0" * (MAX_REQUEST - 53)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    entries = list(vectors())
    inputs = [request(*entry).hex() for entry in entries]
    answers = [expected(*entry) for entry in entries]
    valid_count = len(inputs)
    sample = request(KEY, 123, 1, [["version", "1.2.3"], ["unicode", "\U0001f9d9"]], "test\n")
    for length in range(len(sample)):
        inputs.append(sample[:length].hex())
        answers.append("ERR")
    for bad in [b"\xff", b"\xc0\x80", b"\xed\xa0\x80", b"\xf4\x90\x80\x80", b"\xe2\x82"]:
        payload = request(KEY, 0, 1, [], "")[:49] + struct.pack("<I", len(bad)) + bad
        inputs.append(payload.hex())
        answers.append("ERR")
    for payload in [bytes([2]) + sample[1:], sample + b"\0", bytes(MAX_REQUEST + 1), sample[:45] + struct.pack("<I", 0xFFFFFFFF) + sample[49:]]:
        inputs.append(payload.hex())
        answers.append("ERR")
    data = ("\n".join(inputs) + "\n").encode("ascii")
    result = subprocess.run([args.binary], input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=120)
    if result.returncode:
        raise AssertionError(("oracle process failed", result.returncode, result.stderr.decode(errors="replace")))
    actual = result.stdout.decode("ascii").splitlines()
    if len(actual) != len(answers):
        raise AssertionError(("result count", len(actual), len(answers), result.stderr.decode(errors="replace")))
    for index, (got, want) in enumerate(zip(actual, answers)):
        if got != want:
            raise AssertionError(("oracle mismatch", index, got[:200], want[:200]))
    output = pathlib.Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    expected_data = ("\n".join(answers) + "\n").encode("ascii")
    record = dict(comparisons=len(inputs), valid_vectors=valid_count, malformed_vectors=len(inputs)-valid_count,
                  unicode_scalars=0x110000-0x800, input_sha256=hashlib.sha256(data).hexdigest(),
                  expected_sha256=hashlib.sha256(expected_data).hexdigest(), actual_sha256=hashlib.sha256(result.stdout).hexdigest(),
                  reference="Python stdlib json + hashlib + struct, synthetic NIP-01 inputs", status="passed")
    (output / "oracle.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(record))


if __name__ == "__main__":
    main()
