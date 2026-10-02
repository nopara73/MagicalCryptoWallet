"""Independent Python-stdlib oracle; synthetic data only, not a runtime dependency.

Run via compression_verify.ps1 (actual-source Rust driver). Fixtures and reports
are generated under ignored .artifacts/compression. No packages are installed.
"""
import argparse
import collections
import gzip
import hashlib
import json
import pathlib
import random
import struct
import subprocess
import sys
import zlib


def sha(data):
    return hashlib.sha256(data).hexdigest()


def packet(action, fmt, mode, data, dictionary=None, trailing="reject", members="concat"):
    return "\t".join((action, fmt, mode, data.hex(), "-" if dictionary is None else dictionary.hex(), trailing, members))


def compress(data, fmt, level=6, strategy=zlib.Z_DEFAULT_STRATEGY, dictionary=None, window=15, flush=False):
    wbits = {"raw": -window, "zlib": window, "gzip": window + 16}[fmt]
    kwargs = {} if dictionary is None else {"zdict": dictionary}
    obj = zlib.compressobj(level, zlib.DEFLATED, wbits, 8, strategy, **kwargs)
    if not flush:
        return obj.compress(data) + obj.flush()
    at = len(data) // 3
    end = 2 * len(data) // 3
    return (obj.compress(data[:at]) + obj.flush(zlib.Z_SYNC_FLUSH)
            + obj.compress(data[at:end]) + obj.flush(zlib.Z_FULL_FLUSH)
            + obj.compress(data[end:]) + obj.flush())


def inflate(data, fmt, dictionary=None):
    kwargs = {} if dictionary is None else {"zdict": dictionary}
    obj = zlib.decompressobj({"raw": -15, "zlib": 15, "gzip": 31}[fmt], **kwargs)
    plain = obj.decompress(data) + obj.flush()
    if not obj.eof:
        raise ValueError("truncated stream")
    return plain, len(data) - len(obj.unused_data)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--driver", required=True)
    parser.add_argument("--evidence", required=True)
    args = parser.parse_args()
    root = pathlib.Path(args.evidence)
    root.mkdir(parents=True, exist_ok=True)
    rng = random.Random(0x195019511952)
    payloads = [b"", b"a", b"hello", bytes(range(256)), b"123456789", b"Wikipedia"]
    for n in (258, 32767, 32768, 32769, 65534, 65535, 65536, 131071):
        payloads.append(bytes(i % 251 for i in range(n)))
    for i in range(96):
        n = rng.randrange(0, 18000)
        if i % 4 == 0:
            data = rng.randbytes(n)
        elif i % 4 == 1:
            motif = rng.randbytes(rng.randrange(1, 80))
            data = (motif * (n // len(motif) + 1))[:n]
        elif i % 4 == 2:
            data = bytes(rng.randrange(10) + 48 for _ in range(n))
        else:
            data = (b'{"synthetic":true,"value":12345,"text":"compression"}\n' * (n // 50 + 1))[:n]
        payloads.append(data)

    cases = []
    block_kinds = collections.Counter()
    counts = collections.Counter()

    def add(name, request, expected=None, consumed=None, members=1, encoder=False, error=None, kind="decode"):
        cases.append(dict(name=name, request=request, expected=expected, consumed=consumed,
                          members=members, encoder=encoder, error=error, kind=kind))

    for i, data in enumerate(payloads):
        for fmt in ("raw", "zlib", "gzip"):
            for j in range(2):
                strategy = (zlib.Z_DEFAULT_STRATEGY, zlib.Z_FIXED, zlib.Z_HUFFMAN_ONLY, zlib.Z_RLE)[(i+j) % 4]
                level = (0, 1, 6, 9)[(i*3+j) % 4]
                packed = compress(data, fmt, level, strategy, flush=i % 13 == 0)
                first = {"raw": 0, "zlib": 2, "gzip": 10}[fmt]
                block_kinds[(packed[first] >> 1) & 3] += 1
                # Check the independent generator before submitting to Rust.
                oracle, used = inflate(packed, fmt)
                assert oracle == data and used == len(packed)
                add(f"python-{i}-{fmt}-{j}", packet("D", fmt, "-", packed), data, len(packed))
                if i < 16 or i % 11 == 0:
                    chunks = ("1:1", "2:17", "7:258")[(i+j) % 3]
                    add(f"stream-{i}-{fmt}-{j}", packet("S", fmt, chunks, packed), data, len(packed), kind="stream")
            for method in ("stored", "fixed"):
                add(f"rust-{i}-{fmt}-{method}", packet("E", fmt, method, data), data,
                    len(data), encoder=True, kind="encode")
        if i < 12:
            packed = gzip.compress(data, mtime=0)
            assert gzip.decompress(packed) == data
            add(f"gzip-stdlib-{i}", packet("D", "gzip", "-", packed), data, len(packed))

    # Smaller advertised zlib windows and preset dictionaries, including >32 KiB
    # dictionary tails and overlapping copies into newly emitted output.
    for i in range(32):
        dictionary = rng.randbytes((3, 512, 32768, 70000)[i % 4])
        data = dictionary[-min(1400, len(dictionary)):] * 2 + b"synthetic suffix"
        for fmt in ("raw", "zlib"):
            window = (9, 10, 12, 15)[i % 4]
            packed = compress(data, fmt, 9, dictionary=dictionary, window=window)
            assert inflate(packed, fmt, dictionary)[0] == data
            add(f"dictionary-{i}-{fmt}", packet("D", fmt, "-", packed, dictionary), data, len(packed), kind="dictionary")
            if i < 8:
                add(f"dictionary-stream-{i}-{fmt}", packet("S", fmt, "1:1", packed, dictionary), data, len(packed), kind="stream")
            if fmt == "zlib":
                add(f"dictionary-reject-{i}", packet("D", fmt, "-", packed), error="DictionaryRequired", kind="invalid")
                add(f"dictionary-wrong-{i}", packet("D", fmt, "-", packed, dictionary+b"wrong"), error="DictionaryMismatch", kind="invalid")

    # Independently assembled optional gzip header, with RFC1952 CRC16 from
    # Python's CRC32 and Latin-1 name/comment octets. Metadata is not interpreted.
    for i in range(8):
        data = b"independent optional header " * (i+1)
        basic = gzip.compress(data, mtime=0)
        extra = b"AB" + struct.pack("<H", i+1) + bytes(range(i+1))
        head = b"\x1f\x8b\x08\x1f\0\0\0\0\0\xff" + struct.pack("<H", len(extra)) + extra
        head += b"synthetic-\xe9-name\0comment-\xff\0"
        head += struct.pack("<H", zlib.crc32(head) & 0xffff)
        packed = head + basic[10:]
        assert gzip.decompress(packed) == data
        add(f"header-{i}", packet("S", "gzip", "1:1", packed), data, len(packed), kind="stream")

    for n in (1, 2, 7, 17):
        pieces = [gzip.compress(payloads[i % 12], mtime=0) for i in range(n)]
        packed = b"".join(pieces)
        expected = b"".join(payloads[i % 12] for i in range(n))
        assert gzip.decompress(packed) == expected
        add(f"concat-{n}", packet("D", "gzip", "-", packed), expected, len(packed), n, kind="concat")
        add(f"concat-stream-{n}", packet("S", "gzip", "1:2", packed), expected, len(packed), n, kind="concat")
        add(f"first-{n}", packet("D", "gzip", "-", packed, trailing="allow", members="first"),
            payloads[0], len(pieces[0]), 1, kind="concat")

    # Every byte truncation for independently encoded single-member streams.
    truncations = 0
    for fmt in ("raw", "zlib", "gzip"):
        for level in (0, 6, 9):
            data = b"a valid stream with byte truncations" * 3
            packed = compress(data, fmt, level)
            for cut in range(len(packed)):
                add(f"truncate-{fmt}-{level}-{cut}", packet("D", fmt, "-", packed[:cut]),
                    error="Truncated", kind="invalid")
                truncations += 1
            for tail in (b"arbitrary", b"\0\0", b"\x1fX"):
                add(f"trailing-allow-{fmt}-{level}-{tail.hex()}", packet("D", fmt, "-", packed+tail, trailing="allow"),
                    data, len(packed), kind="trailing")
                add(f"trailing-reject-{fmt}-{level}-{tail.hex()}", packet("D", fmt, "-", packed+tail),
                    error="TrailingData", kind="invalid")

    # Independent random mutations. Accept if zlib recognizes the exact prefix;
    # otherwise require a clean error. Gzip/zlib corruption also checks trailers.
    for fmt in ("raw", "zlib", "gzip"):
        for i in range(100):
            data = payloads[20+i % 50]
            packed = bytearray(compress(data, fmt, 6))
            packed[rng.randrange(len(packed))] ^= 1 << rng.randrange(8)
            packed = bytes(packed)
            try:
                expected, used = inflate(packed, fmt)
            except (zlib.error, ValueError):
                add(f"mutation-{fmt}-{i}", packet("D", fmt, "-", packed, trailing="allow"),
                    error="any", kind="mutation")
            else:
                add(f"mutation-{fmt}-{i}", packet("D", fmt, "-", packed, trailing="allow"),
                    expected, used, kind="mutation")

    assert all(block_kinds[k] > 0 for k in (0, 1, 2)), block_kinds
    fixture_path = root / "reference-fixtures.jsonl"
    with fixture_path.open("w", encoding="utf-8", newline="\n") as f:
        for case in cases:
            record = {k: v for k, v in case.items() if k != "expected"}
            if case["expected"] is not None:
                record["expected_length"] = len(case["expected"])
                record["expected_sha256"] = sha(case["expected"])
            f.write(json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n")

    results_digest = hashlib.sha256()
    for start in range(0, len(cases), 24):
        batch = cases[start:start+24]
        requests = "\n".join(c["request"] for c in batch) + "\n"
        result = subprocess.run([args.driver], input=requests, text=True, capture_output=True, timeout=90)
        if result.returncode:
            raise AssertionError(f"driver failed: {result.stderr}")
        lines = result.stdout.splitlines()
        assert len(lines) == len(batch), (len(lines), len(batch), result.stderr)
        for case, line in zip(batch, lines):
            parts = line.split("\t")
            if case["error"] is not None:
                assert parts[0] == "ERR", (case["name"], line[:200])
                if case["error"] != "any":
                    assert parts[1].startswith(case["error"]), (case["name"], parts)
            else:
                assert parts[0] == "OK", (case["name"], line[:200])
                assert int(parts[1]) == case["consumed"], (case["name"], parts[:3])
                assert int(parts[2]) == case["members"], (case["name"], parts[:3])
                actual = bytes.fromhex(parts[3])
                if case["encoder"]:
                    fmt = case["request"].split("\t")[1]
                    plain, used = inflate(actual, fmt)
                    assert used == len(actual) and plain == case["expected"], case["name"]
                    if fmt == "gzip":
                        assert gzip.decompress(actual) == case["expected"], case["name"]
                else:
                    assert actual == case["expected"], case["name"]
                results_digest.update(sha(actual).encode("ascii"))
            results_digest.update((case["name"] + "\n").encode("utf-8"))
            counts[case["kind"]] += 1

    report = dict(python=sys.version, zlib_build=zlib.ZLIB_VERSION, zlib_runtime=zlib.ZLIB_RUNTIME_VERSION,
                  seed="0x195019511952", payloads=len(payloads), total=len(cases), counts=dict(counts),
                  first_block_kinds={str(k): v for k, v in sorted(block_kinds.items())},
                  truncation_cases=truncations, fixtures=str(fixture_path),
                  fixture_sha256=sha(fixture_path.read_bytes()), results_sha256=results_digest.hexdigest(),
                  reference_source_sha256=sha(pathlib.Path(__file__).read_bytes()),
                  primary_specifications=["https://www.rfc-editor.org/rfc/rfc1950",
                                          "https://www.rfc-editor.org/rfc/rfc1951",
                                          "https://www.rfc-editor.org/rfc/rfc1952"],
                  production_integrated=False, third_party_runtime_dependencies=[])
    (root / "differential.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
