"""Read immutable Git blobs; record retained callers without touching live files."""
import argparse
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().with_name("bitcoin_block_fixtures")

def git(*arguments):
    result = subprocess.run(["git", "-C", str(ROOT), *arguments], capture_output=True, check=False)
    if result.returncode not in (0, 1):
        raise RuntimeError(result.stderr.decode("utf8", "replace"))
    return result.stdout.decode("utf8")

def inventory(revision):
    commit = git("rev-parse", revision).strip()
    patterns = {
        "block_header_merkle": r"\b(Block|BlockHeader|ChainedBlock|ConcurrentChain|HeadersPayload|BlockPayload|MerkleBlock|PartialMerkleTree|MerkleRoot)\b|HashMerkleRoot|CheckProofOfWork|CheckMerkleRoot",
        "nbitcoin_import": r"\busing NBitcoin([.;]|$)",
        "nbitcoin_package": r"NBitcoin(\.Secp256k1)?",
    }
    records, counts = [], {}
    for kind, expression in patterns.items():
        globs = ["*.cs"] if kind != "nbitcoin_package" else ["*.csproj", "Directory.Packages.props", "*packages.lock.json"]
        text = git("grep", "-n", "-I", "-E", expression, commit, "--", *globs)
        found = []
        for row in text.splitlines():
            _, path, line, snippet = row.split(":", 3)
            role = "test_or_tool" if any(x in path for x in ("Tests", "Contrib/", "ThirdParty/")) else "application_or_coordinator"
            found.append((kind, role, path, line, snippet.replace("\t", " ").strip()))
        records.extend(found)
        counts[kind] = {"lines": len(found), "files": len({r[2] for r in found})}
    OUT.mkdir(exist_ok=True)
    (OUT / "managed_callers.tsv").write_text("# immutable_git_revision=" + commit + "\nkind\trole\tpath\tline\tsource\n" + "\n".join("\t".join(row) for row in records) + "\n", newline="\n")
    manifest = {"revision": commit, "counts": counts,
                "direct_partial_merkle_callers": sum("MerkleBlock" in r[4] or "PartialMerkleTree" in r[4] for r in records if r[0] == "block_header_merkle"),
                "remaining_packages": ["NBitcoin 10.0.13", "NBitcoin.Secp256k1 3.1.6"],
                "note": "Mechanical source snapshot, including coordinator/tool roles. Implicit/global imports and all package usages remain; codec migration alone removes no package."}
    (OUT / "inventory.json").write_text(json.dumps(manifest, indent=2) + "\n", newline="\n")
    print(json.dumps(manifest, indent=2))

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--revision", default="HEAD")
    inventory(parser.parse_args().revision)
