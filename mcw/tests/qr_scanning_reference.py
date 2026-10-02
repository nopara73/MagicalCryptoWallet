"""Independent QR encoder oracle. The reference stays in QA evidence, never the app."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import pathlib
import random
import subprocess
import sys
import time

parser = argparse.ArgumentParser()
parser.add_argument("--check", type=pathlib.Path, required=True)
parser.add_argument("--reference", type=pathlib.Path, required=True)
parser.add_argument("--output", type=pathlib.Path, required=True)
parser.add_argument("--replay", type=pathlib.Path)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=False)
reference_hash = hashlib.sha256(args.reference.read_bytes()).hexdigest()
# This is the independent reference pinned in the fixture provenance. Its hash is also
# recorded in the run; callers must not pass a downloaded/unreviewed substitute.
if reference_hash != "b089855caf16185c61421ea4927c1b213cf9468940d71fa8ab11ef83662dcc84":
    raise AssertionError("Independent QR reference hash mismatch")
spec = importlib.util.spec_from_file_location("qr_reference", args.reference)
sys.dont_write_bytecode = True
reference = importlib.util.module_from_spec(spec)
assert spec.loader
spec.loader.exec_module(reference)
Q = reference.QrCode
S = reference.QrSegment
original_placement = Q._draw_codewords


def observed_placement(qr, data):
    # Observe writes made by the independent oracle's actual placement algorithm.
    # This avoids deriving our corruption locations from the Rust decoder itself.
    locations = []

    class Row(list):
        def __init__(self, row, y):
            super().__init__(row)
            self.y = y

        def __setitem__(self, x, value):
            locations.append((x, self.y))
            super().__setitem__(x, value)

    qr._modules = [Row(row, y) for y, row in enumerate(qr._modules)]
    original_placement(qr, data)
    qr._modules = [list(row) for row in qr._modules]
    qr.qa_locations = locations
    qr.qa_codewords = list(data)


Q._draw_codewords = observed_placement
levels = [Q.Ecc.LOW, Q.Ecc.MEDIUM, Q.Ecc.QUARTILE, Q.Ecc.HIGH]
cases = json.loads(args.replay.read_text(encoding="utf-8")) if args.replay else []
rng = random.Random(0x4D43575152)
started = time.monotonic()


def add(qr, text, kind, changes=()):
    modules = [qr.get_module(x, y) for y in range(qr.get_size()) for x in range(qr.get_size())]
    for x, y in changes:
        modules[y * qr.get_size() + x] ^= True
    cases.append(dict(size=qr.get_size(), modules="".join("1" if value else "0" for value in modules),
                      expected=text, kind=kind, version=qr.get_version(), level=qr.get_error_correction_level().ordinal))


for version in ([] if args.replay else range(1, 41)):
    for level in levels:
        for mask in range(8):
            qr = Q.encode_segments([S.make_bytes(b"MCWQR")], level, minversion=version, maxversion=version, mask=mask, boostecl=False)
            add(qr, "MCWQR", "byte-all-versions-levels-masks")
            if mask in (0, 7):
                # Single corrupted codeword, from an independent recorded bit location.
                word = rng.randrange(len(qr.qa_codewords))
                add(qr, "MCWQR", "correct-one-symbol", [qr.qa_locations[word * 8]])
    if version % 10 == 0:
        print(f"Generated through version {version}", flush=True)

for version in ([] if args.replay else (1, 9, 10, 26, 27, 40)):
    for level in levels:
        for segments, text in [
            ([S.make_numeric("000017000")], "000017000"),
            ([S.make_alphanumeric("BTC:00173")], "BTC:00173"),
            ([S.make_eci(26), S.make_bytes("€".encode())], "€"),
            ([S.make_eci(3), S.make_bytes(bytes([0xE9]))], "é"),
            ([S.make_eci(27), S.make_bytes(b"ASCII")], "ASCII"),
        ]:
            qr = Q.encode_segments(segments, level, minversion=version, maxversion=version, mask=3, boostecl=False)
            add(qr, text, "segment-and-version-boundaries")

for level in ([] if args.replay else levels):
    text = "bitcoin:bcrt1q" + "q" * 38 + "?amount=0.002&label=MCW%20synthetic"
    qr = Q.encode_segments([S.make_bytes(text.encode())], level, mask=5, boostecl=False)
    add(qr, text, "synthetic-bip21")
    size = qr.get_size()
    # Copy-independent BCH damage in up to three format bits.
    add(qr, text, "format-correct-three-bits", [(8, 0), (8, 1), (8, 2), (size - 1, 8), (size - 2, 8), (size - 3, 8)])
    # Matrix rotation/mirroring, with independent coordinate permutations.
    for mirror in (False, True):
        for rotation in range(4):
            modules = [[qr.get_module(x, y) for x in range(size)] for y in range(size)]
            if mirror:
                modules = [list(row) for row in zip(*modules)]
            for _ in range(rotation):
                modules = [list(row) for row in zip(*modules[::-1])]
            cases.append(dict(size=size, modules="".join("1" if value else "0" for row in modules for value in row),
                              expected=text, kind="rotate-or-mirror", version=qr.get_version(), level=level.ordinal))

for size in ([] if args.replay else (20, 22, 178)):
    cases.append(dict(size=size, modules="0" * (size * size), expected=None, kind="invalid-dimension"))
if not args.replay:
    cases.append(dict(size=21, modules="0" * 441, expected=None, kind="blank-frame"))
    cases.append(dict(size=21, modules="0" * 440, expected=None, kind="truncated-matrix"))
for version in ([] if args.replay else (1, 5, 10, 40)):
    qr = Q.encode_segments([S.make_eci(999), S.make_bytes(b"MCW")], levels[0], minversion=version, maxversion=version, mask=2, boostecl=False)
    add(qr, None, "unsupported-eci-reject")

# Full error-correction radius, using block parameters and actual placement from
# the independent oracle. Consecutive interleave columns affect the first block.
for version in ([] if args.replay else (1, 3, 5, 10, 25, 40)):
    for level in levels:
        qr = Q.encode_segments([S.make_bytes(b"MCWQR")], level, minversion=version, maxversion=version, mask=4, boostecl=False)
        blocks = Q._NUM_ERROR_CORRECTION_BLOCKS[level.ordinal][version]
        ecc = Q._ECC_CODEWORDS_PER_BLOCK[level.ordinal][version]
        data_total = len(qr.qa_codewords) - blocks * ecc
        short_data = len(qr.qa_codewords) // blocks - ecc

        def first_block_word(column):
            # The final data column is present in long blocks only. Once the
            # first (short) block reaches parity, skip that column entirely.
            if column < short_data:
                return column * blocks
            return data_total + (column - short_data) * blocks

        changes = []
        for index in range(ecc // 2):
            corruption = rng.randrange(1, 256)
            changes.extend(qr.qa_locations[first_block_word(index) * 8 + bit] for bit in range(8) if corruption & (1 << bit))
        add(qr, "MCWQR", "correct-full-block-radius", changes)
        # More than the first block's guaranteed correction radius, with a fixed
        # independent corruption. This corpus must fail, never become an invoice.
        changes.extend(qr.qa_locations[first_block_word(ecc // 2) * 8 + bit] for bit in (0, 3, 7))
        add(qr, None, "uncorrectable-block-reject", changes)

corpus = json.dumps(cases, ensure_ascii=False, separators=(",", ":")).encode()
(args.output / "synthetic-corpus.json").write_bytes(corpus)
inputs = "".join(f"M {case['size']} {case['modules']}\n" for case in cases)
process = subprocess.run([str(args.check.resolve())], input=inputs, text=True, capture_output=True, timeout=120)
(args.output / "decoder-output.txt").write_text(process.stdout, encoding="utf-8")
(args.output / "decoder-errors.txt").write_text(process.stderr, encoding="utf-8")
assert process.returncode == 0, process.stderr
results = process.stdout.splitlines()
assert len(results) == len(cases), (len(results), len(cases))
failures = []
corrected = 0
for index, (case, result) in enumerate(zip(cases, results)):
    if case["expected"] is None:
        passed = result == "ERR"
    else:
        parts = result.split(" ")
        passed = len(parts) == 5 and parts[0] == "OK" and parts[1:3] == [str(case["version"]), str(case["level"])] and bytes.fromhex(parts[4]).decode() == case["expected"]
        if passed:
            corrected += int(parts[3])
    if not passed:
        failures.append(dict(index=index, kind=case["kind"], output=result, expected=case["expected"], version=case.get("version"), level=case.get("level")))
summary = dict(cases=len(cases), passed=len(cases)-len(failures), failed=len(failures), corrected_symbols=corrected,
               oracle_sha256=reference_hash, binary_sha256=hashlib.sha256(args.check.read_bytes()).hexdigest(),
               corpus_sha256=hashlib.sha256(corpus).hexdigest(), elapsed_seconds=round(time.monotonic()-started, 3),
               failures=failures, camera_capture_verified=False, production_payment_authorization=False)
(args.output / "result.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
print(json.dumps({key: value for key, value in summary.items() if key != "failures"}, indent=2), flush=True)
if failures:
    print(json.dumps(failures[:12], indent=2), flush=True)
    raise SystemExit(1)
