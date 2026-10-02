#!/usr/bin/env python3
"""Check single-wallet removals in tracked source, active generated code and package metadata/resources/symbols."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
DATA = {"Contrib/SingleWallet/policy.json", "Contrib/SingleWallet/exceptions.json"}
POLICY = json.loads((HERE / "policy.json").read_text(encoding="utf-8"))
FORBIDDEN = re.compile(POLICY["forbidden_pattern"], re.I)


def line_hash(line):
    return hashlib.sha256(line.encode()).hexdigest()


def check_text(name, text, exceptions):
    failures = []
    for number, line in enumerate(text.splitlines(), 1):
        if FORBIDDEN.search(line) and (name, line_hash(line)) not in exceptions:
            failures.append(f"{name}:{number}: {line[:180]}")
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", nargs="*", default=[])
    args = parser.parse_args()
    recorded = json.loads((HERE / "exceptions.json").read_text(encoding="utf-8"))
    exceptions = {(item["path"], item["line_sha256"]) for item in recorded}
    names = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).decode().split("\0")
    failures, tracked, generated = [], 0, 0
    for name in filter(None, names):
        path = ROOT / name
        if not path.is_file() or name in DATA:
            continue
        tracked += 1
        if FORBIDDEN.search(name):
            failures.append("Retired tracked path: " + name)
        try:
            failures.extend(check_text(name, path.read_text(encoding="utf-8-sig"), exceptions))
        except UnicodeDecodeError:
            pass
    for item in recorded:
        path = ROOT / item["path"]
        if not path.is_file() or item["line_sha256"] not in {line_hash(line) for line in path.read_text(encoding="utf-8-sig").splitlines()}:
            failures.append("Stale exception: " + item["path"])
    for folder in ROOT.glob("MagicalCryptoWallet*/obj"):
        if folder.parent.name.endswith("Tests"):
            continue
        for path in folder.rglob("*.cs"):
            if "GeneratedFiles" in path.parts and not path.parts[path.parts.index("GeneratedFiles") + 1].startswith("MagicalCryptoWallet."):
                continue
            generated += 1
            failures.extend(check_text(path.relative_to(ROOT).as_posix(), path.read_text(encoding="utf-8-sig"), set()))
    for name in args.artifacts:
        folder = Path(name).resolve()
        if not folder.is_dir():
            failures.append("Missing package directory: " + str(folder))
            continue
        for path in folder.rglob("*"):
            if not path.is_file(): continue
            if FORBIDDEN.search(path.relative_to(folder).as_posix()): failures.append("Retired package path: " + str(path))
            if path.suffix in (".json", ".xml", ".axaml", ".config", ".plist", ".desktop"):
                failures.extend(check_text(str(path), path.read_text(encoding="utf-8-sig"), set()))
        subprocess.run(["dotnet", "run", "--project", str(ROOT / "Contrib/Rebrand/AssemblyAudit"), "-c", "Release", "--", str(HERE / "policy.json"), str(folder)], check=True, cwd=ROOT)
    for failure in failures:
        print(failure, file=sys.stderr)
    print(json.dumps(dict(tracked_files=tracked, generated_sources=generated, exact_exceptions=len(recorded), packages=len(args.artifacts), failures=len(failures))))
    return int(bool(failures))


if __name__ == "__main__":
    sys.exit(main())
