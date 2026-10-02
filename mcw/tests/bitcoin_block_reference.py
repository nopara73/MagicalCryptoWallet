"""Offline primary-fixture preparation and independent differential checks.

Only Python stdlib and ignored, hash-pinned Bitcoin Core reference source.
The upstream CBlock/CTransaction/CPartialMerkleTree/CMerkleBlock bodies run
unchanged. Two unused imports for short-id/test utilities and the unrelated
top-level assert_equal call are excluded from that reference module's AST.
No substitute implementation of either imported utility is supplied or used.
"""
from __future__ import annotations

import argparse
import ast
import hashlib
import io
import json
from pathlib import Path
import random
import re
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).resolve().with_name("bitcoin_block_fixtures")
TAG = "v30.0"
PRIMARY = "https://raw.githubusercontent.com/bitcoin/bitcoin/" + TAG + "/"
SEED = 0xB10C37

def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()

def dsha(data: bytes) -> bytes:
    return hashlib.sha256(hashlib.sha256(data).digest()).digest()

def core_reference(directory: Path, sources: dict | None = None) -> dict:
    path = directory / "test_functional_test_framework_messages.py"
    data = path.read_bytes()
    if sources is not None:
        assert sha(data) == sources["test/functional/test_framework/messages.py"]["sha256"]
    parsed = ast.parse(data, filename=str(path))
    parsed.body = [node for node in parsed.body if not (
        isinstance(node, ast.ImportFrom) and node.module and node.module.startswith("test_framework.")
    ) and not (
        isinstance(node, ast.Expr) and isinstance(node.value, ast.Call)
        and isinstance(node.value.func, ast.Name) and node.value.func.id == "assert_equal"
    )]
    namespace = {"__name__": "bitcoin_core_reference"}
    exec(compile(parsed, str(path), "exec"), namespace)
    assert namespace["BLOCK_HEADER_SIZE"] == 80
    return namespace

def compact(value: int) -> bytes:
    if value < 253:
        return bytes([value])
    if value <= 0xffff:
        return b"\xfd" + value.to_bytes(2, "little")
    if value <= 0xffffffff:
        return b"\xfe" + value.to_bytes(4, "little")
    return b"\xff" + value.to_bytes(8, "little")

def merkle(leaves: list[bytes]) -> tuple[bytes, bool]:
    mutated = False
    work = list(leaves)
    while len(work) > 1:
        mutated |= any(work[i] == work[i + 1] for i in range(0, len(work) - 1, 2))
        if len(work) % 2:
            work.append(work[-1])
        work = [dsha(work[i] + work[i + 1]) for i in range(0, len(work), 2)]
    return (work[0] if work else bytes(32)), mutated

def branch(leaves: list[bytes], index: int) -> list[bytes]:
    result = []
    work = list(leaves)
    while len(work) > 1:
        result.append(work[min(index ^ 1, len(work) - 1)])
        if len(work) % 2:
            work.append(work[-1])
        work = [dsha(work[i] + work[i + 1]) for i in range(0, len(work), 2)]
        index //= 2
    return result

def partial(leaves: list[bytes], mask: list[bool]) -> tuple[bytes, bytes, list[int], int]:
    # Independent recursive reference based on Core's traversal specification.
    count = len(leaves)
    width = lambda h: (count + (1 << h) - 1) >> h
    height = 0
    while width(height) > 1:
        height += 1
    bits, hashes, matches = [], [], []
    def node_hash(h, pos):
        if h == 0:
            return leaves[pos]
        left = node_hash(h - 1, pos * 2)
        right = node_hash(h - 1, pos * 2 + 1) if pos * 2 + 1 < width(h - 1) else left
        return dsha(left + right)
    def visit(h, pos):
        matched = any(mask[pos << h:min((pos + 1) << h, count)])
        bits.append(matched)
        if h == 0 or not matched:
            hashes.append(node_hash(h, pos))
            if h == 0 and matched:
                matches.append(pos)
        else:
            visit(h - 1, pos * 2)
            if pos * 2 + 1 < width(h - 1):
                visit(h - 1, pos * 2 + 1)
    visit(height, 0)
    flags = bytearray((len(bits) + 7) // 8)
    for i, bit in enumerate(bits):
        flags[i // 8] |= int(bit) << (i % 8)
    encoded = count.to_bytes(4, "little") + compact(len(hashes)) + b"".join(hashes) + compact(len(flags)) + bytes(flags)
    return encoded, node_hash(height, 0), matches, len(bits)

def target_encode(value: int, negative: bool) -> int:
    exponent = (value.bit_length() + 7) // 8
    word = value << (8 * (3 - exponent)) if exponent <= 3 else value >> (8 * (exponent - 3))
    if word & 0x800000:
        word >>= 8
        exponent += 1
    return word | (exponent << 24) | (0x800000 if negative and word else 0)

def target_decode(bits: int) -> tuple[int, bool, bool, bool, int]:
    exponent, word = bits >> 24, bits & 0x7fffff
    if exponent <= 3:
        word >>= 8 * (3 - exponent)
        value = word
    else:
        value = word << (8 * (exponent - 3))
    negative = bool(word and bits & 0x800000)
    overflow = value.bit_length() > 256
    magnitude = value & ((1 << 256) - 1)
    normalized = target_encode(magnitude, negative)
    return magnitude, negative, overflow, not overflow and normalized == bits, normalized

def source_records(directory: Path) -> dict:
    paths = [
        "src/primitives/block.h", "src/consensus/merkle.cpp", "src/merkleblock.cpp",
        "src/merkleblock.h", "src/arith_uint256.cpp", "src/test/arith_uint256_tests.cpp",
        "src/test/merkle_tests.cpp", "src/test/merkleblock_tests.cpp", "src/test/data/blockfilters.json",
        "src/consensus/consensus.h", "src/kernel/chainparams.cpp", "src/test/util/setup_common.cpp",
        "test/functional/test_framework/messages.py", "COPYING",
    ]
    return {name: {"url": PRIMARY + name, "sha256": sha((directory / name.replace("/", "_")).read_bytes()),
                   "bytes": (directory / name.replace("/", "_")).stat().st_size} for name in paths}

def prepare(directory: Path) -> None:
    sources = source_records(directory)
    core = core_reference(directory)
    raw_vectors = json.loads((directory / "src_test_data_blockfilters.json").read_text())
    candidates = [(f"core-testnet-{row[0]}", row[1], bytes.fromhex(row[2])) for row in raw_vectors[1:]]
    genesis = candidates[0][2]
    for name, timestamp, nonce, bits, expected in [
        ("mainnet-genesis", 1231006505, 2083236893, 0x1d00ffff, "000000000019d6689c085ae165831e934ff763ae46a2a6c172b3f1b60a8ce26f"),
        ("signet-genesis", 1598918400, 52613770, 0x1e0377ae, "00000008819873e925422c1ff0f99f7cc9bbb232af63a077a480a3633bee1ef6"),
        ("regtest-genesis", 1296688602, 2, 0x207fffff, "0f9188f13cb7b2c71f2a335e3a4fc328bf5beb436012afca590b1a11466e2206"),
    ]:
        chainparams = (directory / "src_kernel_chainparams.cpp").read_text()
        assert expected in chainparams and f"{timestamp}, {nonce}, 0x{bits:08x}" in chainparams
        candidates.append((name, expected, genesis[:68] + struct.pack("<III", timestamp, bits, nonce) + genesis[80:]))
    setup = (directory / "src_test_util_setup_common.cpp").read_text()
    excerpt = setup[setup.index("CBlock getBlock13b8a()") :]
    nine = bytes.fromhex(re.search(r'"([0-9a-f]+)"_hex', excerpt).group(1))
    candidates.append(("core-merkleblock-nine", "0000000000013b8ab2cd513b0261a14096412195a72a0c4827d229dcc7e0f7af", nine))
    rows, partial_rows, witness_count = [], [], 0
    for name, expected, raw in candidates:
        block = core["CBlock"]()
        stream = io.BytesIO(raw)
        block.deserialize(stream)
        assert stream.tell() == len(raw) and block.serialize() == raw
        assert block.hash_hex == expected
        leaves = [core["ser_uint256"](tx.txid_int) for tx in block.vtx]
        wtxids = [core["ser_uint256"](tx.wtxid_int) for tx in block.vtx]
        witness = sum(tx.serialize_with_witness() != tx.serialize_without_witness() for tx in block.vtx)
        witness_count += witness
        root, mutated = merkle(leaves)
        assert not mutated and root == raw[36:68]
        assert block.calc_merkle_root() == int.from_bytes(root, "little")
        stripped, total = len(block.serialize(with_witness=False)), len(raw)
        rows.append("\t".join([name, expected, str(len(leaves)), str(witness), str(stripped), str(total), str(stripped * 3 + total), raw.hex(), ",".join(x.hex() for x in leaves), ",".join(x.hex() for x in wtxids)]))
        masks = [[False] * len(leaves), [True] * len(leaves)]
        if len(leaves) > 1:
            masks.append([i in (1, len(leaves) - 1) for i in range(len(leaves))])
        for mask in masks:
            encoded, got_root, matched, bits = partial(leaves, mask)
            reference = core["CPartialMerkleTree"]()
            reference.deserialize(io.BytesIO(encoded))
            assert reference.serialize() == encoded
            mb = core["CMerkleBlock"]()
            mb.deserialize(io.BytesIO(raw[:80] + encoded))
            assert mb.serialize() == raw[:80] + encoded
            partial_rows.append("\t".join([name, "".join("1" if bit else "0" for bit in mask), "".join(x.hex() for x in leaves), encoded.hex(), got_root.hex(), ",".join(map(str, matched)), str(bits), raw[:80].hex()]))
    if witness_count == 0:
        raise AssertionError("Witness reference coverage missing")
    FIXTURES.mkdir(exist_ok=True)
    (FIXTURES / "blocks.tsv").write_text("# name\thash_display\ttx_count\twitness_count\tstripped\ttotal\tweight\tblock_hex\ttxids_raw\twtxids_raw\n" + "\n".join(rows) + "\n", newline="\n")
    (FIXTURES / "partial.tsv").write_text("# name\tmask\tleaves_raw\ttree_hex\troot_raw\tmatched_indices\tbits_used\theader_hex\n" + "\n".join(partial_rows) + "\n", newline="\n")
    (FIXTURES / "BITCOIN-CORE-LICENSE.txt").write_bytes((directory / "COPYING").read_bytes())
    manifest = {"reference": "Bitcoin Core " + TAG, "sources": sources,
                "block_vectors": len(rows), "partial_vectors": len(partial_rows), "witness_transactions": witness_count,
                "files": {name: sha((FIXTURES / name).read_bytes()) for name in ("blocks.tsv", "partial.tsv", "BITCOIN-CORE-LICENSE.txt")}}
    (FIXTURES / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", newline="\n")
    print(json.dumps({k: manifest[k] for k in ("block_vectors", "partial_vectors", "witness_transactions")}, indent=2))

def differential(directory: Path, executable: Path, evidence: Path) -> None:
    manifest = json.loads((FIXTURES / "manifest.json").read_text())
    for name, expected in manifest["files"].items():
        assert sha((FIXTURES / name).read_bytes()) == expected
    core = core_reference(directory, manifest["sources"])
    randomizer = random.Random(SEED)
    requests, expected, counts = [], [], {}
    def case(kind, request, result):
        counts[kind] = counts.get(kind, 0) + 1
        requests.append(request)
        expected.append("OK|" + result)
    def boolean(value):
        return str(bool(value)).lower()
    for line in (FIXTURES / "blocks.tsv").read_text().splitlines():
        if line.startswith("#"):
            continue
        f = line.split("\t")
        case("primary_block", "B|" + f[7], "|".join([f[7], f[1], f[2], f[4], f[5], f[6], f[7][72:136], "false", f[8], f[9]]))
    for _ in range(256):
        raw = randomizer.randbytes(80)
        header = core["CBlockHeader"]()
        header.deserialize(io.BytesIO(raw))
        assert header.serialize() == raw
        case("header", "H|" + raw.hex(), raw.hex() + "|" + header.hash_hex)
    for n in list(range(1, 65)) + [127, 128, 129, 252, 253, 254, 255, 256, 257, 511, 512, 513, 1024]:
        leaves = [randomizer.randbytes(32) for _ in range(n)]
        joined = b"".join(leaves).hex()
        root, mutated = merkle(leaves)
        assert core["CBlock"].get_merkle_root(list(leaves)) == int.from_bytes(root, "little")
        case("merkle", "M|" + joined, root.hex() + "|" + boolean(mutated))
        for index in sorted({0, n // 2, n - 1}):
            siblings = branch(leaves, index)
            case("proof", f"P|{joined}|{index}", root.hex() + "|false|" + ",".join(x.hex() for x in siblings))
        masks = [[False] * n, [True] * n, [randomizer.randrange(4) == 0 for _ in range(n)]]
        for mask in masks:
            encoded, root, selected, bits = partial(leaves, mask)
            upstream = core["CPartialMerkleTree"]()
            upstream.deserialize(io.BytesIO(encoded))
            assert upstream.serialize() == encoded
            result = "|".join([encoded.hex(), root.hex(), ",".join(map(str, selected)), str(bits), "true"])
            case("partial_build", "T|" + joined + "|" + "".join("1" if bit else "0" for bit in mask), result)
            case("partial_decode", "E|" + encoded.hex(), result)
        if n > 1 and n & (n - 1):
            suffix = n & -n
            expanded = leaves + leaves[-suffix:]
            other_root, mutated = merkle(expanded)
            assert other_root == root and mutated
            case("merkle_mutation", "M|" + b"".join(expanded).hex(), root.hex() + "|true")
    for exponent in range(256):
        for word in (0, 1, 0xff, 0x100, 0xffff, 0x10000, 0x7fffff, 0x800000, 0x800001, 0xffffff):
            bits = (exponent << 24) | word
            magnitude, negative, overflow, canonical, normalized = target_decode(bits)
            case("compact_target_boundary", f"C|{bits:08x}", f"{magnitude:064x}|{boolean(negative)}|{boolean(overflow)}|{boolean(canonical)}|{normalized:08x}")
    for _ in range(4096):
        bits = randomizer.getrandbits(32)
        magnitude, negative, overflow, canonical, normalized = target_decode(bits)
        case("compact_target_random", f"C|{bits:08x}", f"{magnitude:064x}|{boolean(negative)}|{boolean(overflow)}|{boolean(canonical)}|{normalized:08x}")
    for _ in range(512):
        magnitude, negative = randomizer.getrandbits(256), bool(randomizer.getrandbits(1))
        case("compact_target_encode", f"TC|{magnitude:064x}|{int(negative)}", f"{target_encode(magnitude, negative):08x}")
    for line in (FIXTURES / "partial.tsv").read_text().splitlines():
        if line.startswith("#"):
            continue
        f = line.split("\t")
        case("primary_merkleblock", "MB|" + f[7] + f[3], "|".join([f[7] + f[3], f[4], f[5], f[6], "true"]))
    # One process, no shell; synthetic lines carry only public fixtures/random data.
    completed = subprocess.run([str(executable)], input="\n".join(requests) + "\n", text=True, capture_output=True, check=True)
    outputs = completed.stdout.splitlines()
    assert len(outputs) == len(expected), (len(outputs), len(expected), completed.stderr)
    for index, (actual, wanted) in enumerate(zip(outputs, expected)):
        if actual != wanted:
            raise AssertionError(f"Differential case {index}: {requests[index][:160]}\n{actual[:800]}\nexpected {wanted[:800]}")
    result = {"seed": hex(SEED), "passed": len(expected), "counts": counts,
              "driver": str(executable.resolve()), "driver_sha256": sha(executable.read_bytes()),
              "reference_messages_sha256": manifest["sources"]["test/functional/test_framework/messages.py"]["sha256"],
              "production_release": False, "platform": "x86_64-pc-windows-msvc"}
    evidence.mkdir(parents=True, exist_ok=True)
    (evidence / "differential.json").write_text(json.dumps(result, indent=2) + "\n", newline="\n")
    print(json.dumps(result, indent=2))

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("prepare", "differential"))
    parser.add_argument("--references", type=Path, required=True)
    parser.add_argument("--driver", type=Path)
    parser.add_argument("--evidence", type=Path, default=ROOT / ".artifacts/bitcoin-block-evidence")
    args = parser.parse_args()
    if args.mode == "prepare":
        prepare(args.references)
    else:
        if not args.driver:
            parser.error("--driver is required for differential verification")
        differential(args.references, args.driver, args.evidence)
