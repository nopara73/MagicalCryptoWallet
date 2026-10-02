"""Inventory immutable managed source references and retained synthetic fixtures.

Only reads committed source blobs; never reads wallets, keys or user data. Broad
mechanical matches include tests and comments and are not a reachability proof.
"""
import hashlib
import io
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "mcw/tests/bitcoin_script_fixtures"
REVISION = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip()
NAMES = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", "-z", REVISION], cwd=ROOT).decode().split("\0")
selected = [name for name in sorted(filter(None, NAMES)) if not name.startswith("mcw/") and (
    Path(name).suffix in {".cs", ".csproj", ".props"} or Path(name).name == "packages.lock.json")]
requests = "".join(f"{REVISION}:{name}\n" for name in selected).encode()
stream = io.BytesIO(subprocess.check_output(["git", "cat-file", "--batch"], input=requests, cwd=ROOT))
patterns = [
    ("script_format", re.compile(r"\b(?:Script|ScriptPubKey|ScriptSig|WitScript|OpcodeType)\b|GetDestinationAddress|IsScriptType|PayTo\w+Template|GetScriptPubKey|ExtractKeyId"), {".cs"}),
    ("script_validation", re.compile(r"VerifyScript|ScriptVerify|ScriptEvaluationContext|SignatureHash|GetSignatureHash"), {".cs"}),
    ("package", re.compile(r'(?:PackageReference|PackageVersion) Include="NBitcoin[^\"]*"'), {".csproj", ".props"}),
    ("package_lock", re.compile(r'"NBitcoin(?:\.Secp256k1)?"'), {".json"}),
]
rows = []
fixtures = []
file_hashes = {}
for name in selected:
    header = stream.readline().split()
    assert len(header) == 3 and header[1] == b"blob", name
    content = stream.read(int(header[2]))
    assert stream.read(1) == b"\n"
    text = content.decode("utf-8-sig")
    matched = False
    for number, line in enumerate(text.splitlines(), 1):
        for category, pattern, extensions in patterns:
            if Path(name).suffix in extensions and pattern.search(line):
                rows.append(f"{category}\t{name}\t{number}\t{line.strip().replace(chr(9), r'\t')}")
                matched = True
    if matched:
        file_hashes[name] = hashlib.sha256(content).hexdigest()
    if Path(name).suffix != ".cs":
        continue
    # Literal-only fixtures. Interpolated or dynamically assembled inputs are
    # intentionally excluded. Concatenated hex string literals are preserved.
    expression = r'(?:Script\.FromHex|new Script)\(\s*((?:"[^"\r\n]*"\s*\+\s*)*"[^"\r\n]*")\s*\)'
    for match in re.finditer(expression, text):
        value = "".join(re.findall(r'"([^"]*)"', match[1]))
        dialect = "hex" if match[0].startswith("Script.FromHex") else "wallet"
        if dialect == "hex" and not re.fullmatch(r"[0-9a-fA-F]*", value):
            continue
        line = text.count("\n", 0, match.start()) + 1
        fixtures.append(f"{name}:{line}\t{dialect}\t{value.encode().hex()}")
OUT.mkdir(exist_ok=True)
(OUT / "managed_callers.tsv").write_text(f"# snapshot_revision={REVISION}\n# category\tpath\tline\tsource\n" +
                                         "\n".join(rows) + "\n", encoding="utf-8", newline="\n")
(OUT / "application_fixtures.tsv").write_text("# source:line\tdialect\tinput_utf8_hex\n" + "\n".join(fixtures) + "\n", encoding="utf-8", newline="\n")
counts = {category: {"references": sum(row.startswith(category + "\t") for row in rows),
                    "files": len({row.split("\t")[1] for row in rows if row.startswith(category + "\t")})}
          for category, _, _ in patterns}
manifest = {"snapshot_revision": REVISION, "counts": counts, "fixtures": len(fixtures),
            "source_hashes": file_hashes, "package_removal": False,
            "scan_note": "Explicit committed matches include tests/comments; implicit imports and runtime reachability require further audit."}
(OUT / "inventory.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8", newline="\n")
print(json.dumps({"snapshot_revision": REVISION, "counts": counts, "fixtures": len(fixtures)}))
