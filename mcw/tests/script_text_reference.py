"""Legacy Script TEXT compatibility: actual retained assembly + synthetic cases.

The .NET oracle is development-only and uses the already-retained cached package.
Fixture preparation requires that exact assembly; normal Rust fixture checks do
not require it. No third-party code is imported into the Rust implementation.
"""
import argparse
import hashlib
import json
import random
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIXTURES = HERE / "script_text_fixtures"
SEED = 0x0D082026


def synthetic_cases(seed=SEED, random_count=4000):
    rng = random.Random(seed)
    cases = []
    add_parse = lambda text: cases.append(("P", text.encode("utf-8")))
    add_render = lambda data: cases.append(("R", data))
    basic = ["", "0", "1", "10", "16", "-1", "81", "00", "01", "OP_0", "OP_TRUE", "OP_FALSE", "true", "false", "OP_1NEGATE", "OP_10", "OP_16", "OP_DUP", "DUP", "op_dup", "OP_HODL", "OP_CLTV", "OP_CSV", "OP_NOP2", "OP_NOP3", "OP_CHECKLOCKTIMEVERIFY", "OP_CHECKSEQUENCEVERIFY", "OP_INVALIDOPCODE", "OP_CHECKSIGADD", "OP_PUSHDATA1", "0x01"]
    for text in basic:
        add_parse(text)
    for code in range(256):
        add_render(bytes([code]))
        for suffix in ("", ")", ")extra", "anything", "\u2003)"):
            add_parse(f"OP_UNKNOWN(0x{code:02x}{suffix}")
    for suffix in ("", ")", "0)", "g0)", "0g)", "\u0100)", "\U0001f600)"):
        add_parse("OP_UNKNOWN(0x" + suffix)
    # Every ASCII boundary/interior byte, plus the full .NET whitespace set and
    # representative non-whitespace Unicode characters. Unknown suffixes above
    # independently cover Unicode inside the permissively ignored suffix.
    chars = [chr(n) for n in range(128)]
    chars += [chr(n) for n in [0x85, 0xA0, 0x1680, *range(0x2000, 0x200B), 0x2028, 0x2029, 0x202F, 0x205F, 0x3000, 0x180E, 0x200B, 0xFEFF, 0x3A9, 0x1F600]]
    for ch in chars:
        add_parse(ch + "OP_DUP" + ch)
        add_parse("OP_DUP" + ch + "OP_HASH160")
    payloads = [b"", b"\0", b"\x80", b"\x81", b"\x01\0", b"\0\0", b"\xab"]
    payloads += [bytes([n]) for n in range(1, 17)]
    payloads += [rng.randbytes(n) for n in (20, 32, 33, 65, 75, 76, 255, 256, 520, 521, 4096)]
    for data in payloads:
        if data:
            add_parse(data.hex())
        prefixes = []
        if len(data) <= 75:
            prefixes.append(bytes([len(data)]))
        if len(data) <= 255:
            prefixes.append(b"\x4c" + bytes([len(data)]))
        prefixes += [b"\x4d" + len(data).to_bytes(2, "little"), b"\x4e" + len(data).to_bytes(4, "little")]
        for prefix in prefixes:
            add_render(prefix + data)
            add_render(b"\x76" + prefix + data)
            for removed in range(1, min(len(prefix + data), 8) + 1):
                add_render((prefix + data)[:-removed])
    for declared in (0, 1, 20, 75, 76, 255, 256, 520, 521, 65535, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF):
        prefix = b"\x4e" + declared.to_bytes(4, "little")
        for n in range(1, 5):
            add_render(prefix[:n])
        for body in (b"", b"\x01", b"\x76\xab\xff"):
            add_render(prefix + body)
            add_render(b"\x51\x76" + prefix + body)
    tokens = basic + [f"OP_UNKNOWN(0x{n:02x})ignored" for n in range(256)]
    spaces = [" ", "\t", "\n", "\r", "\v", "\f"]
    for _ in range(random_count):
        add_render(rng.randbytes(rng.randrange(0, 97)))
        text = rng.choice(spaces).join(rng.choice(tokens) for _ in range(rng.randrange(0, 16)))
        add_parse(rng.choice(["", "\u00a0", "\u2003"]) + text + rng.choice(["", "\u00a0", "\u2003"]))
    return cases


def run(command, cases):
    result = subprocess.run(command, input="".join(f"{op}\t{data.hex()}\n" for op, data in cases), text=True, encoding="utf-8", capture_output=True, check=True)
    rows = result.stdout.splitlines()
    if len(rows) != len(cases):
        raise AssertionError(f"Expected {len(cases)} responses; got {len(rows)}. {result.stderr}")
    if any(row != "ERR" and not row.startswith("OK:") for row in rows):
        raise AssertionError("Unexpected reference/probe response")
    return rows


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def prepare(oracle, assembly):
    cases = synthetic_cases()
    expected = run(["dotnet", str(oracle)], cases)
    FIXTURES.mkdir(parents=True, exist_ok=True)
    target = FIXTURES / "vectors.tsv"
    target.write_text("# operation\tinput_hex\texpected\t.\n" + "".join(f"{op}\t{data.hex()}\t{result}\t.\n" for (op, data), result in zip(cases, expected)), encoding="utf-8", newline="\n")
    manifest = {
        "reference": "NBitcoin 10.0.13 net10.0, retained application package, test-only",
        "repository_commit": "bd666454562c12155210fac65b07c70289dcecc1",
        "oracle_assembly_sha256": sha(assembly),
        "oracle_program_sha256": sha(HERE / "script_text_reference/Program.cs"),
        "generator_sha256": sha(__file__),
        "seed": SEED,
        "cases": len(cases),
        "parse_cases": sum(op == "P" for op, _ in cases),
        "render_cases": sum(op == "R" for op, _ in cases),
        "parse_errors": sum(op == "P" and result == "ERR" for (op, _), result in zip(cases, expected)),
        "fixtures_sha256": sha(target),
        "synthetic_only": True,
        "production_release": False,
    }
    (FIXTURES / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps(manifest, indent=2))


def compare(probe, oracle, output):
    # Fresh separate seed; expected results come from the actual retained library,
    # never from the Rust implementation or the preparation fixture results.
    cases = synthetic_cases(SEED ^ 0xFEED, 3000)
    expected = run(["dotnet", str(oracle)], cases)
    actual = run([str(probe)], cases)
    for i, ((op, data), old, new) in enumerate(zip(cases, expected, actual)):
        if old != new:
            raise AssertionError(f"Case {i} {op} {data.hex()}: NBitcoin {old}, Rust {new}")
    report = {"checks": len(cases), "parse_cases": sum(op == "P" for op, _ in cases), "render_cases": sum(op == "R" for op, _ in cases), "seed": SEED ^ 0xFEED, "probe_sha256": sha(probe), "oracle_sha256": sha(oracle), "synthetic_only": True, "production_release": False}
    Path(output).write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--assembly", type=Path)
    parser.add_argument("--probe", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.prepare:
        if args.assembly is None:
            parser.error("--assembly is required for preparation")
        prepare(args.oracle, args.assembly)
    else:
        if args.probe is None or args.output is None:
            parser.error("--probe and --output are required for comparison")
        compare(args.probe, args.oracle, args.output)
