"""Record retained managed references from committed source, without wallet data.

This is a mechanical audit, not proof that every imported type is used or that
implicit type references are absent. Global using directives are recorded too.
"""
import re
import io
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[2]
revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root).decode().strip()
files = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", "-z", revision], cwd=root).decode().split("\0")
patterns = [
    ("direct_nbitcoin", re.compile(r"\bNBitcoin\b"), {".cs"}),
    ("address_codec", re.compile(r"BitcoinAddress|GetDestinationAddress|Bitcoin(?:WitPubKey|WitScript|PubKey|Script|Taproot)Address|Bech32Encoder|Encoders\.(?:Hex|Base58|Base58Check)|TryParseBitcoinAddressForNetwork"), {".cs"}),
    ("package", re.compile(r'(?:PackageReference|PackageVersion) Include="NBitcoin[^"]*"'), {".csproj", ".props"}),
    ("package_lock", re.compile(r'"NBitcoin(?:\.Secp256k1)?"'), {".json"}),
]
rows = []
selected_files = []
for name in sorted(filter(None, files)):
    if name.startswith("mcw/"):
        continue
    path = Path(name)
    if path.suffix not in {".cs", ".csproj", ".props", ".json"}:
        continue
    if path.suffix == ".json" and path.name != "packages.lock.json":
        continue
    selected_files.append(name)
# Read immutable blobs in one batch, so the recorded revision is exact even in
# a checkout where another task has unrelated uncommitted changes.
requests = "".join(f"{revision}:{name}\n" for name in selected_files).encode()
blobs = io.BytesIO(subprocess.check_output(["git", "cat-file", "--batch"], input=requests, cwd=root))
for name in selected_files:
    header = blobs.readline().split()
    if len(header) != 3 or header[1] != b"blob":
        raise RuntimeError(f"Cannot read committed source {name}")
    content = blobs.read(int(header[2]))
    if blobs.read(1) != b"\n":
        raise RuntimeError(f"Invalid git batch framing for {name}")
    path = Path(name)
    lines = content.decode("utf-8-sig").splitlines()
    for line_number, line in enumerate(lines, 1):
        for category, regex, extensions in patterns:
            if path.suffix in extensions and regex.search(line):
                # Keep exact source context, escaping separators in this TSV.
                excerpt = line.strip().replace("\t", "\\t")
                rows.append(f"{category}\t{name}\t{line_number}\t{excerpt}")
out = root / "mcw/tests/bitcoin_encoding_fixtures/managed_callers.tsv"
out.write_text(f"# snapshot_revision={revision}\n# category\tpath\tline\tsource\n" + "\n".join(rows) + "\n", encoding="utf-8")
for category, _, _ in patterns:
    selected = [row.split("\t") for row in rows if row.startswith(category + "\t")]
    print(f"{category}: {len(selected)} references in {len({row[1] for row in selected})} files")
print(f"Snapshot {revision}")
