"""Independent raster corpus from the verified matrix oracle, never bundled."""
from __future__ import annotations
import argparse
import hashlib
import json
import math
import pathlib
import random
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument("--check", type=pathlib.Path, required=True)
parser.add_argument("--corpus", type=pathlib.Path, required=True)
parser.add_argument("--output", type=pathlib.Path, required=True)
parser.add_argument("--replay", type=pathlib.Path)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=False)
source = json.loads(args.corpus.read_text(encoding="utf-8"))
cases = json.loads(args.replay.read_text(encoding="utf-8")) if args.replay else []
for case in cases:
    if hashlib.sha256(pathlib.Path(case["path"]).read_bytes()).hexdigest() != case["sha256"]:
        raise AssertionError("Independent image fixture changed")
rng = random.Random(0x4D4357494D47)

def add(name, width, height, pixels, expected):
    path = args.output / (name + ".pgm")
    path.write_bytes(f"P5\n{width} {height}\n255\n".encode() + bytes(pixels))
    cases.append(dict(path=str(path.resolve()), expected=expected, kind=name,
                      sha256=hashlib.sha256(path.read_bytes()).hexdigest()))

def render(case, scale=3, angle=0, shadow=False, mirror=False):
    size = case["size"]
    edge = (size + 8) * scale
    side = int(edge * 1.45) + 16 if angle else edge
    pixels = bytearray([245]) * (side * side)
    sine, cosine = math.sin(math.radians(angle)), math.cos(math.radians(angle))
    for y in range(side):
        for x in range(side):
            px, py = x + .5 - side / 2, y + .5 - side / 2
            u = (cosine * px + sine * py + edge / 2) / scale - 4
            v = (-sine * px + cosine * py + edge / 2) / scale - 4
            if mirror:
                u = size - u
            dark = 0 <= u < size and 0 <= v < size and case["modules"][int(v) * size + int(u)] == "1"
            value = 18 if dark else 245
            if shadow:
                value = int(value * (.35 + .65 * x / side))
            pixels[y * side + x] = value
    return side, side, pixels

for version in ([] if args.replay else range(1, 41)):
    case = next(c for c in source if c.get("kind") == "byte-all-versions-levels-masks"
                and c["version"] == version and c["level"] == version % 4)
    add(f"planar-v{version:02}", *render(case), case["expected"])

for version in ([] if args.replay else (1, 3, 5, 10, 25, 40)):
    case = next(c for c in source if c.get("kind") == "byte-all-versions-levels-masks"
                and c["version"] == version and c["level"] == 2)
    for angle in (17, 45, 103):
        add(f"rotated-v{version:02}-{angle}", *render(case, scale=4, angle=angle), case["expected"])
    add(f"shadow-v{version:02}", *render(case, scale=4, shadow=True), case["expected"])
    add(f"mirror-v{version:02}", *render(case, scale=4, mirror=True), case["expected"])

for size in ([] if args.replay else (64, 256)):
    add(f"blank-{size}", size, size, [255] * (size * size), None)
    add(f"noise-{size}", size, size, [rng.randrange(256) for _ in range(size * size)], None)

(args.output / "corpus.json").write_text(json.dumps(cases, indent=2), encoding="utf-8")
started = time.monotonic()
run = subprocess.run([str(args.check)], input="\n".join("I " + c["path"] for c in cases) + "\n",
                     text=True, capture_output=True, timeout=len(cases) * 2 + 20, check=True)
(args.output / "decoder-output.txt").write_text(run.stdout, encoding="utf-8")
lines = run.stdout.splitlines()
failures = []
for case, line in zip(cases, lines, strict=True):
    actual = bytes.fromhex(line.split()[-1]).decode("utf-8") if line.startswith("OK ") else None
    if actual != case["expected"]:
        failures.append(dict(kind=case["kind"], expected=case["expected"], actual=actual))
result = dict(cases=len(cases), passed=len(cases)-len(failures), failures=failures,
              elapsed_seconds=round(time.monotonic()-started,3),
              binary_sha256=hashlib.sha256(args.check.read_bytes()).hexdigest(),
              matrix_corpus_sha256=hashlib.sha256(args.corpus.read_bytes()).hexdigest(),
              camera_capture_verified=False, perspective_capture_verified=False)
(args.output / "result.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
print(json.dumps(result, indent=2), flush=True)
if failures:
    raise SystemExit(1)
