"""Record retained managed references from committed source, without wallet data.

This is a mechanical audit, not proof that every imported type is used or that
implicit type references are absent. Global using directives are recorded too.
"""
import re
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parents[2]
files = subprocess.check_output(["git", "ls-files", "-z"], cwd=root).decode().split("\0")
revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root).decode().strip()
patterns = [
    ("direct_nbitcoin", re.compile(r"\bNBitcoin\b"), {".cs"}),
    ("address_codec", re.compile(r"BitcoinAddress|GetDestinationAddress|Bitcoin(?:WitPubKey|WitScript|PubKey|Script|Taproot)Address|Bech32Encoder|Encoders\.(?:Hex|Base58|Base58Check)|TryParseBitcoinAddressForNetwork"), {".cs"}),
    ("package", re.compile(r'(?:PackageReference|PackageVersion) Include="NBitcoin[^"]*"'), {".csproj", ".props"}),
    ("package_lock", re.compile(r'"NBitcoin(?:\.Secp256k1)?"'), {".json"}),
]
rows = []
for name in sorted(filter(None, files)):
    if name.startswith("mcw/"):
        continue
    path = root / name
    if not path.is_file():
        continue
    if path.suffix not in {".cs", ".csproj", ".props", ".json"}:
        continue
    if path.suffix == ".json" and path.name != "packages.lock.json":
        continue
    lines = path.read_text(encoding="utf-8-sig").splitlines()
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
