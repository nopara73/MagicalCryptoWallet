"""Non-shipping independent PNG/zlib oracle; requires an existing Pillow install.

The Rust implementation has no dependency on this script, Python, Pillow or zlib.
Run after compiling png_contract.rs with rustc --test. All fixture data is synthetic.
"""

import argparse
import binascii
import csv
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import zlib

from PIL import Image, __version__ as pillow_version


def inspect_png(path, modules, width, height, scale, depth):
    data = path.read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    cursor, chunks, stream, ihdr = 8, [], bytearray(), None
    while cursor < len(data):
        length = struct.unpack_from(">I", data, cursor)[0]
        assert length <= 0x7FFFFFFF
        kind = data[cursor + 4 : cursor + 8]
        end = cursor + 8 + length
        assert end + 4 <= len(data)
        body = data[cursor + 8 : end]
        checksum = struct.unpack_from(">I", data, end)[0]
        assert checksum == binascii.crc32(data[cursor + 4 : end])
        chunks.append(kind)
        if kind == b"IHDR":
            ihdr = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            stream.extend(body)
        elif kind == b"IEND":
            assert length == 0
        cursor = end + 4
    assert cursor == len(data)
    assert chunks == [b"IHDR", b"IDAT", b"IEND"]
    pixel_width, pixel_height = (width + 8) * scale, (height + 8) * scale
    assert ihdr == (pixel_width, pixel_height, depth, 0, 0, 0, 0)
    assert len(modules) == width * height and set(modules) <= {0, 1}
    assert stream[:2] == b"\x78\x01"
    assert (stream[0] * 256 + stream[1]) % 31 == 0

    # This inflater is independent of both first-party Rust encoder and decoder.
    inflater = zlib.decompressobj()
    raw = inflater.decompress(bytes(stream)) + inflater.flush()
    assert inflater.eof and not inflater.unused_data and not inflater.unconsumed_tail
    assert zlib.adler32(raw) == struct.unpack(">I", stream[-4:])[0]
    row_bytes = (pixel_width + 7) // 8 if depth == 1 else pixel_width
    assert len(raw) == (row_bytes + 1) * pixel_height
    assert len(stream) == len(raw) + 5 * ((len(raw) + 65534) // 65535) + 6

    # Inspect block structure separately: terminal bits, LEN/NLEN and boundaries.
    block_cursor, block_count, stored = 2, 0, bytearray()
    while True:
        flag = stream[block_cursor]
        assert flag in (0, 1)
        count, inverse = struct.unpack_from("<HH", stream, block_cursor + 1)
        assert count ^ inverse == 65535
        if flag == 0:
            assert count == 65535
        block_cursor += 5
        stored.extend(stream[block_cursor : block_cursor + count])
        block_cursor += count
        block_count += 1
        if flag == 1:
            break
    assert block_cursor + 4 == len(stream)
    assert stored == raw
    assert block_count == (len(raw) + 65534) // 65535

    quiet = 4 * scale
    white_row = bytes([255]) * pixel_width
    expected = bytearray(white_row * quiet)
    for y in range(height):
        row = bytearray([255]) * quiet
        for module in modules[y * width : (y + 1) * width]:
            row.extend(bytes([0 if module else 255]) * scale)
        row.extend(bytes([255]) * quiet)
        expected.extend(row * scale)
    expected.extend(white_row * quiet)

    # Full image decoder: dimensions, bit depth, opacity, every pixel and orientation.
    with Image.open(path) as image:
        assert image.format == "PNG" and image.size == (pixel_width, pixel_height)
        assert image.mode == ("1" if depth == 1 else "L")
        assert "transparency" not in image.info
        image.load()
        assert image.convert("L").tobytes() == expected
        assert image.convert("RGBA").getchannel("A").getextrema() == (255, 255)

    for y in range(pixel_height):
        offset = y * (row_bytes + 1)
        assert raw[offset] == 0
        if depth == 1 and pixel_width % 8:
            assert raw[offset + row_bytes] & ((1 << (8 - pixel_width % 8)) - 1) == 0
    return {
        "name": path.name,
        "sha256": hashlib.sha256(data).hexdigest(),
        "png_bytes": len(data),
        "raw_bytes": len(raw),
        "deflate_blocks": block_count,
        "width": pixel_width,
        "height": pixel_height,
        "depth": depth,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--encoder-tests", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    output = args.output_dir.resolve()
    # Preserve the rule that test files and fixture output stay in this checkout.
    checkout = Path(__file__).resolve().parents[2]
    artifacts = checkout / ".artifacts"
    assert output.is_relative_to(artifacts), "output must be under this checkout's .artifacts"
    assert args.encoder_tests.resolve().is_relative_to(artifacts)
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, MCW_PNG_EVIDENCE_DIR=str(output))
    subprocess.run(
        [str(args.encoder_tests.resolve()), "--ignored", "--exact", "emit_oracle_fixtures", "--nocapture"],
        env=env,
        check=True,
    )
    cases = []
    with (output / "fixtures.tsv").open(newline="", encoding="utf-8") as manifest:
        for item in csv.DictReader(manifest, delimiter="\t"):
            name = item["name"]
            assert name.startswith("fixture-") and all(c.isdigit() or c in "fixture-" for c in name)
            width, height, scale, depth = (int(item[k]) for k in ("module_width", "module_height", "scale", "depth"))
            assert (int(item["pixel_width"]), int(item["pixel_height"])) == ((width + 8) * scale, (height + 8) * scale)
            cases.append(inspect_png(output / f"{name}.png", (output / f"{name}.modules").read_bytes(), width, height, scale, depth))
    assert len(cases) == 270, f"expected all 270 conformance fixtures, got {len(cases)}"
    report = {
        "state": "passed",
        "fixtures": len(cases),
        "pillow_version": pillow_version,
        "zlib_version": zlib.ZLIB_VERSION,
        "total_png_bytes": sum(case["png_bytes"] for case in cases),
        "checks": ["PNG chunk order, lengths and CRC32", "zlib header and Adler32", "independent zlib inflate", "DEFLATE stored block boundaries and final flag", "Pillow decode, opacity and every pixel", "four-module margins, integral scaling and orientation", "packed final-byte padding"],
        "cases": cases,
    }
    (output / "verification.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"PASS: {len(cases)} independently decoded PNGs; Pillow {pillow_version}, zlib {zlib.ZLIB_VERSION}")
    print(f"Evidence: {output / 'verification.json'}")


if __name__ == "__main__":
    main()
