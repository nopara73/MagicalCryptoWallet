"""Development-only independent transaction/signature-hash oracle.

Python arbitrary-precision integers, hashlib and independently decoded raw wire
transactions. Upstream expected hashes/messages are checked, not regenerated from
the Rust implementation. No signing key material is saved in derived fixtures.
"""
import argparse
import hashlib
import json
import random
import re
import struct
import subprocess
from pathlib import Path
from bitcoin_script_reference import ROOT, FIXTURES, SOURCES, parse


def compact(n):
    return bytes([n]) if n < 253 else b"\xfd" + n.to_bytes(2, "little") if n <= 65535 else b"\xfe" + n.to_bytes(4, "little") if n <= 0xffffffff else b"\xff" + n.to_bytes(8, "little")


def decode(raw):
    pos = 0

    def read(n):
        nonlocal pos
        data = raw[pos:pos+n]
        if len(data) != n:
            raise ValueError("truncated wire")
        pos += n
        return data

    def count():
        tag = read(1)[0]
        return int.from_bytes(read({253: 2, 254: 4, 255: 8}[tag]), "little") if tag >= 253 else tag

    def blob():
        size = count()
        if size > len(raw):
            raise ValueError("invalid blob size")
        return read(size)

    version = read(4)
    n = count()
    witness = n == 0
    if witness:
        if read(1) != b"\x01":
            raise ValueError("not tx")
        n = count()
    if n > 100000:
        raise ValueError("input count")
    inputs = [(read(36), blob(), read(4)) for _ in range(n)]
    n = count()
    if n > 100000:
        raise ValueError("output count")
    outputs = [(read(8), blob()) for _ in range(n)]
    if witness:
        for _ in inputs:
            for _ in range(count()):
                blob()
    locktime = read(4)
    if pos != len(raw):
        raise ValueError("not exact tx")
    return version, inputs, outputs, locktime


def out_bytes(output):
    value, script = output
    return value + compact(len(script)) + script


def tx_bytes(tx):
    version, inputs, outputs, locktime = tx
    return version + compact(len(inputs)) + b"".join(outpoint + compact(len(script)) + script + sequence for outpoint, script, sequence in inputs) + compact(len(outputs)) + b"".join(out_bytes(output) for output in outputs) + locktime


def sha(data):
    return hashlib.sha256(data).digest()


def double(data):
    return sha(sha(data))


def legacy(tx, index, script, hash_type):
    version, inputs, outputs, locktime = tx
    if hash_type & 31 == 3 and index >= len(outputs):
        return bytes([1]) + bytes(31)
    tokens, failure = parse(script)
    if failure:
        raise ValueError("malformed legacy scriptCode")
    code = b"".join(raw for _, op, _, raw in tokens if op != 171)
    modified = [(outpoint, code if i == index else b"", bytes(4) if i != index and hash_type & 31 in (2, 3) else sequence)
                for i, (outpoint, _, sequence) in enumerate(inputs)]
    if hash_type & 128:
        modified = [modified[index]]
    if hash_type & 31 == 2:
        outputs = []
    elif hash_type & 31 == 3:
        outputs = [(bytes([255]) * 8, b"")] * index + [outputs[index]]
    return double(tx_bytes((version, modified, outputs, locktime)) + hash_type.to_bytes(4, "little"))


def segwit(tx, index, script, amount, hash_type):
    version, inputs, outputs, locktime = tx
    base = hash_type & 31
    anyone = bool(hash_type & 128)
    prevouts = bytes(32) if anyone else double(b"".join(x[0] for x in inputs))
    sequences = bytes(32) if anyone or base in (2, 3) else double(b"".join(x[2] for x in inputs))
    hashed_outputs = double(b"".join(out_bytes(x) for x in outputs)) if base not in (2, 3) else (
        double(out_bytes(outputs[index])) if base == 3 and index < len(outputs) else bytes(32))
    outpoint, _, sequence = inputs[index]
    preimage = version + prevouts + sequences + outpoint + compact(len(script)) + script + amount.to_bytes(8, "little", signed=True) + sequence + hashed_outputs + locktime + hash_type.to_bytes(4, "little")
    return double(preimage)


def tagged(tag, msg):
    prefix = sha(tag)
    return sha(prefix + prefix + msg)


def taproot(tx, spent, index, hash_type, annex=None, extension=None):
    version, inputs, outputs, locktime = tx
    anyone = bool(hash_type & 128)
    base = 1 if hash_type == 0 else hash_type & 3
    if base == 3 and index >= len(outputs):
        return "single", None
    msg = bytes([0, hash_type]) + version + locktime
    if not anyone:
        msg += sha(b"".join(x[0] for x in inputs)) + sha(b"".join(x[0] for x in spent))
        msg += sha(b"".join(compact(len(x[1])) + x[1] for x in spent)) + sha(b"".join(x[2] for x in inputs))
    if base == 1:
        msg += sha(b"".join(out_bytes(x) for x in outputs))
    msg += bytes([2 * int(extension is not None) + int(annex is not None)])
    msg += inputs[index][0] + out_bytes(spent[index]) + inputs[index][2] if anyone else index.to_bytes(4, "little")
    if annex is not None:
        msg += sha(compact(len(annex)) + annex)
    if base == 3:
        msg += sha(out_bytes(outputs[index]))
    if extension is not None:
        leaf, separator = extension
        msg += leaf + bytes([0]) + separator.to_bytes(4, "little")
    return tagged(b"TapSighash", msg), msg


def spent_text(spent):
    return ";".join(str(int.from_bytes(value, "little", signed=True)) + ":" + script.hex() for value, script in spent)


def prepare():
    core = json.loads((SOURCES / "core-sighash.json").read_text())
    rows = []
    for index, (raw, script, input_index, hash_type, expected_display) in enumerate(core[1:]):
        hash_type &= 0xffffffff
        expected_raw = bytes.fromhex(expected_display)[::-1]
        assert legacy(decode(bytes.fromhex(raw)), input_index, bytes.fromhex(script), hash_type) == expected_raw
        rows.append(f"{index}\t{raw}\t{script}\t{input_index}\t{hash_type}\t{expected_raw.hex()}\t.")
    (FIXTURES / "legacy_sighash.tsv").write_text("# id\ttx_hex\tscript_code_hex\tinput\thash_type_u32\traw_digest_hex\tend\n" + "\n".join(rows) + "\n", encoding="utf-8", newline="\n")
    bip341 = json.loads((SOURCES / "bip341-wallet-vectors.json").read_text())
    rows341 = []
    for vector in bip341["keyPathSpending"]:
        raw = vector["given"]["rawUnsignedTx"]
        tx = decode(bytes.fromhex(raw))
        spent = [(x["amountSats"].to_bytes(8, "little", signed=True), bytes.fromhex(x["scriptPubKey"])) for x in vector["given"]["utxosSpent"]]
        for row in vector["inputSpending"]:
            index = row["given"]["txinIndex"]
            hash_type = row["given"]["hashType"]
            digest, message = taproot(tx, spent, index, hash_type)
            assert digest.hex() == row["intermediary"]["sigHash"]
            assert message.hex() == row["intermediary"]["sigMsg"]
            rows341.append(f"{raw}\t{spent_text(spent)}\t{index}\t{hash_type}\t{message.hex()}\t{digest.hex()}\t.")
    (FIXTURES / "taproot_sighash.tsv").write_text("# tx_hex\tspent_outputs\tinput\thash_type\tepoch_message_hex\traw_digest_hex\tend\n" + "\n".join(rows341) + "\n", encoding="utf-8", newline="\n")
    # Find actual raw txs in BIP143 examples, not preimages or signing keys.
    bip143 = (SOURCES / "bip143.mediawiki").read_text(encoding="utf-8")
    candidates = []
    for match in re.finditer(r"(?<![0-9a-f])([0-9a-f]{100,})(?![0-9a-f])", bip143):
        try:
            raw = bytes.fromhex(match[1]); tx = decode(raw)
            if tx[1] and tx[2]:
                candidates.append((raw, tx))
        except (ValueError, IndexError, OverflowError):
            continue
    rows143 = []
    for match in re.finditer(r"hash preimage(?: for [A-Z|]+)?:\s*([0-9a-f]+)", bip143):
        preimage = bytes.fromhex(match[1])
        assert preimage[104] < 253
        code_size = preimage[104]
        script = preimage[105:105 + code_size]
        amount = int.from_bytes(preimage[105 + code_size:113 + code_size], "little", signed=True)
        hash_type = int.from_bytes(preimage[-4:], "little")
        expected = double(preimage)
        published = re.search(r"sighash:\s*([0-9a-f]{64})", bip143[match.end():], re.IGNORECASE)
        assert published and expected.hex() == published[1]
        found = None
        for raw, tx in candidates:
            for index, (outpoint, _, sequence) in enumerate(tx[1]):
                if outpoint == preimage[68:104] and sequence == preimage[113 + code_size:117 + code_size] and segwit(tx, index, script, amount, hash_type) == expected:
                    found = (raw, index); break
            if found:
                break
        assert found, "BIP143 preimage transaction binding"
        raw, index = found
        rows143.append(f"{raw.hex()}\t{script.hex()}\t{index}\t{amount}\t{hash_type}\t{expected.hex()}\t.")
    assert len(rows143) >= 8
    (FIXTURES / "segwit_sighash.tsv").write_text("# tx_hex\tscript_code_hex\tinput\tamount\thash_type\traw_digest_hex\tend\n" + "\n".join(rows143) + "\n", encoding="utf-8", newline="\n")
    manifest = {"legacy_vectors": len(rows), "segwit_vectors": len(rows143), "taproot_vectors": len(rows341),
                "hashes": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in [SOURCES / "core-sighash.json", SOURCES / "bip143.mediawiki", SOURCES / "bip341.mediawiki", SOURCES / "bip341-wallet-vectors.json", FIXTURES / "legacy_sighash.tsv", FIXTURES / "segwit_sighash.tsv", FIXTURES / "taproot_sighash.tsv"]},
                "scope": "published digest/message expectations; no key/signature verification claim"}
    (FIXTURES / "sighash_manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps({k: v for k, v in manifest.items() if k != "hashes"}))


def differential(binary, output):
    rng = random.Random(0x143341)
    requests = []; expected = []
    for _ in range(1200):
        count = rng.randrange(1, 8)
        tx = (rng.randbytes(4), [(rng.randbytes(36), b"", rng.randbytes(4)) for _ in range(count)],
              [(rng.randrange(0, 2100000000000001).to_bytes(8, "little"), rng.randbytes(rng.randrange(0, 100))) for _ in range(rng.randrange(0, 8))], rng.randbytes(4))
        raw = tx_bytes(tx).hex()
        index = rng.randrange(count)
        script = b"\x02\xab\x42\xab\x51"  # embedded data 0xab plus a real separator
        amount = rng.randrange(0, 2100000000000001)
        hash_type = rng.choice([0, 1, 2, 3, 0x81, 0x82, 0x83, rng.randrange(0, 1 << 32)])
        requests.append(f"L\t{raw}\t{script.hex()}\t{index}\t{hash_type}"); expected.append(legacy(tx, index, script, hash_type).hex())
        requests.append(f"S\t{raw}\t{script.hex()}\t{index}\t{amount}\t{hash_type}"); expected.append(segwit(tx, index, script, amount, hash_type).hex())
        spent = [(amount.to_bytes(8, "little"), b"\x51\x20" + rng.randbytes(32)) for _ in range(count)]
        hash_type = rng.choice([0, 1, 2, 3, 0x81, 0x82, 0x83])
        annex = b"\x50" + rng.randbytes(rng.randrange(0, 80)) if rng.randrange(2) else None
        extension = (rng.randbytes(32), rng.randrange(0, 1 << 32)) if rng.randrange(2) else None
        digest, _ = taproot(tx, spent, index, hash_type, annex, extension)
        requests.append(f"T\t{raw}\t{spent_text(spent)}\t{index}\t{hash_type}\t{annex.hex() if annex is not None else '-'}\t{extension[0].hex() if extension else '-'}\t{extension[1] if extension else 0}")
        expected.append(digest.hex() if isinstance(digest, bytes) else digest)
    result = subprocess.run([str(binary)], input="\n".join(requests) + "\n", text=True, encoding="utf-8", capture_output=True, check=True)
    actual = result.stdout.splitlines()
    assert len(actual) == len(expected), (len(actual), len(expected), result.stderr[:200])
    for i, (got, wanted) in enumerate(zip(actual, expected)):
        if got != wanted:
            raise AssertionError(f"sighash differential {i}: actual {got} != {wanted}; request {requests[i][:150]}")
    evidence = {"checks": len(expected), "seed": "0x143341", "probe_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "oracle_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), "production_release": False}
    output.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(evidence))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--probe", type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / ".artifacts/bitcoin-script-evidence/sighash-differential.json")
    args = parser.parse_args()
    if args.prepare: prepare()
    if args.probe: differential(args.probe, args.output)
