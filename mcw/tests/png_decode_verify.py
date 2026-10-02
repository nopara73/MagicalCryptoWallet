"""Non-shipping independent PNG input conformance and real-fixture verification.

Uses installed Python zlib/Pillow as test oracles, never as a runtime dependency.
Optional PngSuite downloads use HTTPS into the checkout's ignored fixture cache.
"""

import argparse
import binascii
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import re
import struct
import subprocess
from urllib.parse import urljoin, urlparse
from urllib.request import urlopen
import zlib

from PIL import Image, __version__ as pillow_version

PATTERN = (
    (1, 6, 4, 6, 2, 6, 4, 6), (7,) * 8,
    (5, 6, 5, 6, 5, 6, 5, 6), (7,) * 8,
    (3, 6, 4, 6, 3, 6, 4, 6), (7,) * 8,
    (5, 6, 5, 6, 5, 6, 5, 6), (7,) * 8,
)
COMBINATIONS = [(0, d) for d in (1, 2, 4, 8, 16)] + [(2, 8), (2, 16)] + [
    (3, d) for d in (1, 2, 4, 8)
] + [(4, 8), (4, 16), (6, 8), (6, 16)]
SOURCE_CHANNELS = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", binascii.crc32(kind + data))


def png_bytes(width, height, color, depth, interlace, raw, before=(), after=(), method="dynamic"):
    if method == "fixed":
        coder = zlib.compressobj(6, zlib.DEFLATED, 15, 8, zlib.Z_FIXED)
        compressed = coder.compress(raw) + coder.flush()
    else:
        compressed = zlib.compress(raw, 0 if method == "stored" else 9)
    # Deliberately split zlib header, data and trailer across consecutive IDATs.
    pieces = [compressed[:1], b"", compressed[1:-1], compressed[-1:]]
    header = struct.pack(">IIBBBBB", width, height, depth, color, 0, 0, interlace)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + b"".join(before) + b"".join(
        chunk(b"IDAT", part) for part in pieces
    ) + b"".join(after) + chunk(b"IEND", b"")


def pack(samples, depth):
    if depth == 16:
        return b"".join(struct.pack(">H", sample) for sample in samples)
    if depth == 8:
        return bytes(samples)
    packed = bytearray((len(samples) * depth + 7) // 8)
    for i, value in enumerate(samples):
        shift = 8 - depth - (i * depth) % 8
        packed[(i * depth) // 8] |= value << shift
    return bytes(packed)


def filter_row(row, previous, bpp, mode):
    result = bytearray([mode])
    for i, value in enumerate(row):
        left = row[i - bpp] if i >= bpp else 0
        up = previous[i] if previous else 0
        corner = previous[i - bpp] if previous and i >= bpp else 0
        if mode == 0:
            predictor = 0
        elif mode == 1:
            predictor = left
        elif mode == 2:
            predictor = up
        elif mode == 3:
            predictor = (left + up) // 2
        else:
            p = left + up - corner
            candidates = [left, up, corner]
            predictor = min(candidates, key=lambda candidate: abs(p - candidate))
        result.append((value - predictor) & 255)
    return bytes(result)


def raw_passes(samples, width, height, color, depth, interlace, filter_mode):
    raw = bytearray()
    channels = SOURCE_CHANNELS[color]
    bpp = (channels * depth + 7) // 8
    # Independent pass classification uses the standard 8x8 pass-number map,
    # rather than the production decoder's x/y/step tuples.
    for pass_number in range(1, 8) if interlace else (0,):
        previous = b""
        for y in range(height):
            selected = []
            for x in range(width):
                if not interlace or PATTERN[y % 8][x % 8] == pass_number:
                    selected.extend(samples[(y * width + x) * channels : (y * width + x + 1) * channels])
            if selected:
                row = pack(selected, depth)
                raw.extend(filter_row(row, previous, bpp, filter_mode))
                previous = row
    return bytes(raw)


def downsample(value, depth):
    return value >> 8 if depth == 16 else value if depth == 8 else value * 255 // ((1 << depth) - 1)


def original_pixels(samples, color, depth, palette, transparency):
    channels = SOURCE_CHANNELS[color]
    output_channels = 1 if color == 0 and transparency is None else 3 if color in (2, 3) and transparency is None else 4
    output = bytearray()
    for offset in range(0, len(samples), channels):
        source = samples[offset : offset + channels]
        if color == 3:
            index = source[0]
            output.extend(palette[index * 3 : index * 3 + 3])
            if output_channels == 4:
                output.append(transparency[index] if index < len(transparency) else 255)
        elif color in (0, 4):
            value = downsample(source[0], depth)
            output.extend([value] * (1 if output_channels == 1 else 3))
            if output_channels == 4:
                output.append(downsample(source[1], depth) if color == 4 else 0 if source[0] == transparency else 255)
        else:
            output.extend(downsample(value, depth) for value in source[:3])
            if output_channels == 4:
                output.append(downsample(source[3], depth) if color == 6 else 0 if tuple(source) == transparency else 255)
    return bytes(output), output_channels


def parts(data):
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    cursor, found = 8, []
    while cursor < len(data):
        length = struct.unpack_from(">I", data, cursor)[0]
        assert length <= 0x7FFFFFFF
        kind = data[cursor + 4 : cursor + 8]
        end = cursor + 8 + length
        body = data[cursor + 8 : end]
        assert end + 4 <= len(data)
        assert struct.unpack_from(">I", data, end)[0] == binascii.crc32(kind + body)
        found.append((kind, body))
        cursor = end + 4
    assert cursor == len(data) and found[0][0] == b"IHDR" and found[-1] == (b"IEND", b"")
    return found


def pillow_pixels(data, channels):
    with Image.open(io.BytesIO(data)) as image:
        image.load()
        if image.mode in ("I", "I;16", "I;16B"):
            # Pillow exposes full 16-bit grayscale samples. Reduce them explicitly
            # instead of its generic convert('L'), which clips at 255.
            key = image.info.get("transparency")
            pixels = bytearray()
            for value in image.get_flattened_data():
                pixels.extend([int(value) >> 8] * (1 if channels == 1 else 3))
                if channels == 4:
                    pixels.append(0 if value == key else 255)
            return bytes(pixels), image.size
        if data[25] == 0 and channels == 4:
            # Pillow expands packed grayscale to L but keeps the tRNS key in
            # its original sample depth. Apply that key in the expanded domain;
            # generic convert('RGBA') otherwise misses transparency at depth 4.
            key = downsample(image.info["transparency"], data[24])
            pixels = bytearray()
            for value in image.convert("L").get_flattened_data():
                pixels.extend((value, value, value, 0 if value == key else 255))
            return bytes(pixels), image.size
        return image.convert({1: "L", 3: "RGB", 4: "RGBA"}[channels]).tobytes(), image.size


def make_synthetic(output, add):
    sequence = 0
    for color, depth in COMBINATIONS:
        for width, height in ((1, 1), (2, 3), (3, 2), (9, 1), (1, 9), (17, 13)):
            maximum = (1 << depth) - 1
            sample_count = width * height * SOURCE_CHANNELS[color]
            samples = [((i * 173 + i // 3 * 37 + 19) & maximum) for i in range(sample_count)]
            palette = bytes((i * 71 + 13) & 255 for i in range((1 << depth) * 3)) if color == 3 else b""
            expected, channels = original_pixels(samples, color, depth, palette, None)
            for interlace in (0, 1):
                for mode in range(5):
                    raw = raw_passes(samples, width, height, color, depth, interlace, mode)
                    before = [chunk(b"PLTE", palette)] if palette else []
                    for method in ("stored", "fixed", "dynamic"):
                        data = png_bytes(width, height, color, depth, interlace, raw, before, method=method)
                        oracle, size = pillow_pixels(data, channels)
                        assert size == (width, height) and oracle == expected
                        add(f"synthetic-{sequence:04}", data, expected, width, height, channels, "pillow+original-raster")
                        sequence += 1
    # Explicit transparency and non-premultiplied alpha. Complete 16-bit keys
    # must match before sample reduction; Pillow's 8-bit RGB conversion cannot
    # distinguish values sharing a high byte, so original source samples are the
    # independent oracle for that particular precision boundary.
    for color, depth in [(0, 1), (0, 2), (0, 4), (0, 8), (0, 16), (2, 8), (2, 16), (3, 1), (3, 2), (3, 4), (3, 8)]:
        maximum = (1 << depth) - 1
        if color == 3:
            samples = [0, 1, 0, 1, 0, 1]
            palette = bytes((i * 67 + 1) & 255 for i in range((1 << depth) * 3))
            transparency = bytes([0])
            trns = transparency
        else:
            channels = SOURCE_CHANNELS[color]
            samples = [(i * 79 + 0x1234) & maximum for i in range(6 * channels)]
            if depth == 16:
                samples[channels] = samples[0] ^ 1
            palette = b""
            transparency = samples[0] if color == 0 else tuple(samples[:3])
            trns = struct.pack(">H", transparency) if color == 0 else struct.pack(">HHH", *transparency)
        before = ([chunk(b"PLTE", palette)] if palette else []) + [chunk(b"tRNS", trns)]
        expected, channels = original_pixels(samples, color, depth, palette, transparency)
        for interlace in (0, 1):
            raw = raw_passes(samples, 3, 2, color, depth, interlace, 4)
            data = png_bytes(3, 2, color, depth, interlace, raw, before)
            oracle, size = pillow_pixels(data, channels)
            assert size == (3, 2)
            source = "original-raster+Pillow-structure" if color == 2 and depth == 16 else "pillow+original-raster"
            if source == "pillow+original-raster":
                assert oracle == expected, (color, depth, interlace, oracle[:32], expected[:32])
            add(f"transparency-{color}-{depth}-{interlace}", data, expected, 3, 2, channels, source)
    # Legal high-ratio uniform image: exact image-derived limits bound expansion.
    data = png_bytes(1024, 1024, 0, 8, 0, (b"\0" + b"\xff" * 1024) * 1024)
    expected = b"\xff" * (1024 * 1024)
    assert pillow_pixels(data, 1)[0] == expected
    add("high-ratio-uniform", data, expected, 1024, 1024, 1, "pillow+original-raster")


def make_invalid(add):
    good = png_bytes(1, 1, 0, 8, 0, b"\0\xff")
    invalid = {
        "bad-signature": b"bad PNG!" + good[8:],
        "bad-crc": good[:29] + bytes([good[29] ^ 1]) + good[30:],
        "trailing-file": good + b"trailing",
        "missing-end": good[:-12],
        "unknown-critical": png_bytes(1, 1, 0, 8, 0, b"\0\xff", [chunk(b"ABCD", b"")]),
        "reserved-chunk-bit": png_bytes(1, 1, 0, 8, 0, b"\0\xff", [chunk(b"abcc", b"")]),
        "animated": png_bytes(1, 1, 0, 8, 0, b"\0\xff", [chunk(b"acTL", b"\0" * 8)]),
        "bad-filter": png_bytes(1, 1, 0, 8, 0, b"\x05\xff"),
        "wrong-raw-size": png_bytes(2, 2, 0, 8, 0, b"\0\xff"),
        "oversized-raw": png_bytes(1, 1, 0, 8, 0, b"\0\xff\0"),
        "palette-index": png_bytes(1, 1, 3, 8, 0, b"\0\x01", [chunk(b"PLTE", b"\x01\x02\x03")]),
        "palette-missing": png_bytes(1, 1, 3, 8, 0, b"\0\0"),
        "duplicate-header": png_bytes(1, 1, 0, 8, 0, b"\0\0", [chunk(b"IHDR", parts(good)[0][1])]),
        "illegal-alpha-key": png_bytes(1, 1, 6, 8, 0, b"\0" * 5, [chunk(b"tRNS", b"\0\0")]),
    }
    header = parts(good)[0][1]
    stream = zlib.compress(b"\0\xff")
    invalid["nonconsecutive-idat"] = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", stream[:1]) + chunk(b"tEXt", b"key\0value") + chunk(b"IDAT", stream[1:]) + chunk(b"IEND", b"")
    coder = zlib.compressobj(zdict=b"synthetic dictionary")
    preset = coder.compress(b"\0\xff") + coder.flush()
    invalid["preset-dictionary"] = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", preset) + chunk(b"IEND", b"")
    corrupted_stream = stream[:-1] + bytes([stream[-1] ^ 1])
    invalid["adler-mismatch"] = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", corrupted_stream) + chunk(b"IEND", b"")
    invalid["trailing-zlib"] = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", stream + b"unused") + chunk(b"IEND", b"")
    for name, data in invalid.items():
        add("invalid-" + name, data, None, 0, 0, 0, "adversarial")


def external_pixels(data):
    entries = parts(data)
    width, height, depth, color, _, _, _ = struct.unpack(">IIBBBBB", entries[0][1])
    trns = any(kind == b"tRNS" for kind, _ in entries)
    channels = 1 if color == 0 and not trns else 3 if color in (2, 3) and not trns else 4
    pixels, size = pillow_pixels(data, channels)
    assert size == (width, height)
    return pixels, width, height, channels


def download_pngsuite(output, add):
    index_url = "https://www.libpng.org/pub/png/pngsuite.html"
    cache = output / "downloaded-pngsuite"
    cache.mkdir(exist_ok=True)
    with urlopen(index_url, timeout=30) as response:
        page = response.read(1024 * 1024).decode("utf-8", "replace")
    urls = sorted({urljoin(index_url, src) for src in re.findall(r'<img\b[^>]*\bsrc\s*=\s*["\']([^"\']+)', page, flags=re.I) if src.lower().endswith(".png")})
    # The page includes a suite icon; only corpus files in its PNG suite directory.
    urls = [url for url in urls if "/pngsuite/" in urlparse(url).path]
    assert urls, "primary PngSuite index exposed no PNG fixtures"
    for index, url in enumerate(urls):
        name = Path(urlparse(url).path).name
        destination = cache / name
        if not destination.exists():
            with urlopen(url, timeout=30) as response:
                data = response.read(32 * 1024 * 1024 + 1)
                assert len(data) <= 32 * 1024 * 1024
                declared = response.headers.get("Content-Length")
                if declared:
                    assert int(declared) == len(data), "incomplete background download"
            destination.write_bytes(data)
        data = destination.read_bytes()
        if name.startswith("x"):
            add("pngsuite-" + destination.stem, data, None, 0, 0, 0, url)
        else:
            expected, width, height, channels = external_pixels(data)
            add("pngsuite-" + destination.stem, data, expected, width, height, channels, url)
        if index % 25 == 0:
            print(f"PngSuite background downloads verified: {index + 1}/{len(urls)}", flush=True)
    return len(urls)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--decoder-tests", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--download-pngsuite", action="store_true")
    args = parser.parse_args()
    output = args.output_dir.resolve()
    checkout = Path(__file__).resolve().parents[2]
    assert output.is_relative_to(checkout / ".artifacts")
    assert args.decoder_tests.resolve().is_relative_to(checkout / ".artifacts")
    output.mkdir(parents=True, exist_ok=True)
    records, report_cases = [], []

    def add(name, data, expected, width, height, channels, oracle):
        assert all(c.isalnum() or c in "-_" for c in name)
        (output / f"{name}.png").write_bytes(data)
        if expected is not None:
            assert len(expected) == width * height * channels
            (output / f"{name}.pixels").write_bytes(expected)
        records.append((name, "accept" if expected is not None else "reject", width, height, channels))
        report_cases.append({"name": name, "expectation": records[-1][1], "sha256": hashlib.sha256(data).hexdigest(), "oracle": oracle})

    make_synthetic(output, add)
    make_invalid(add)
    # Real existing camera/image QR fixtures are synthetic repository test data.
    real = checkout / "MagicalCryptoWallet.Tests" / "UnitTests" / "QrDecode" / "QrResources"
    real_count = 0
    for source in sorted(real.glob("*.png")):
        data = source.read_bytes()
        expected, width, height, channels = external_pixels(data)
        add("real-" + source.stem, data, expected, width, height, channels, "repository synthetic QR fixture + Pillow")
        real_count += 1
    pngsuite_count = download_pngsuite(output, add) if args.download_pngsuite else 0
    with (output / "fixtures.tsv").open("w", newline="", encoding="utf-8") as manifest:
        writer = csv.writer(manifest, delimiter="\t", lineterminator="\n")
        writer.writerow(("name", "expectation", "width", "height", "channels"))
        writer.writerows(records)
    env = dict(os.environ, MCW_PNG_DECODE_FIXTURES=str(output))
    subprocess.run([str(args.decoder_tests.resolve()), "--ignored", "--exact", "decode_oracle_fixtures", "--nocapture"], env=env, check=True)
    result = (output / "rust-decoder-result.txt").read_text().strip().split("\t")
    accepted = sum(row[1] == "accept" for row in records)
    rejected = len(records) - accepted
    assert result == ["passed", str(accepted), str(rejected)]
    report = {"state": "passed", "accepted": accepted, "rejected": rejected, "pillow": pillow_version, "zlib": zlib.ZLIB_VERSION, "real_repository_pngs": real_count, "pngsuite_downloads": pngsuite_count, "cases": report_cases}
    (output / "verification.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"PASS PNG input: {accepted} exact decoded images; {rejected} invalid/unsupported images rejected; {real_count} real QR fixtures; {pngsuite_count} PngSuite downloads")


if __name__ == "__main__":
    main()
