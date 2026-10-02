"""Bind Markdown verification to exact source bytes and the immutable corpus."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
LEGACY = ROOT / "mcw/tests/native_ui_markdown_legacy.json"
LEGACY_SHA256 = "02e8b976e6671d70f07eca2c354b0014c2486065c8f9ef9d3b01ffb72328002e"
DIRECTORIES = [
    "mcw/src", "MagicalCryptoWallet", "MagicalCryptoWallet.Client",
    "MagicalCryptoWallet.Fluent", "MagicalCryptoWallet.Fluent.Generators",
    "ThirdParty/WabiSabi/csharp/WabiSabi",
    "Contrib/McwMigration/NativeUiVerification", "Contrib/McwMigration/NativeUiBridgeProbe",
]
SUFFIXES = {".rs", ".cs", ".csproj", ".axaml", ".props", ".targets"}


def digest(path):
    raw = path.read_bytes()
    normalized = raw.replace(b"\r\n", b"\n")
    return {"sha256": hashlib.sha256(raw).hexdigest(),
            "lf_sha256": hashlib.sha256(normalized).hexdigest(),
            "git_blob": hashlib.sha1(b"blob " + str(len(normalized)).encode() + b"\0" + normalized).hexdigest()}


def sources():
    for directory in DIRECTORIES:
        for path in (ROOT / directory).rglob("*"):
            relative = path.relative_to(ROOT)
            if path.is_file() and not {"bin", "obj", "native_ui"}.intersection(relative.parts):
                if path.suffix in SUFFIXES or path.name in {"packages.lock.json", "ReleaseHighlights.md"}:
                    yield path
    for name in ["Directory.Build.props", "Directory.Build.targets", "Directory.Packages.props",
                 "global.json", "NuGet.Config", "BannedSymbols.txt", "mcw/Cargo.toml",
                 "mcw/Cargo.lock", "mcw/build.rs", "mcw/rust-toolchain.toml"]:
        path = ROOT / name
        if path.is_file():
            yield path
    for path in (ROOT / "mcw/tests").glob("native_ui_markdown_*"):
        if path.is_file() and path.suffix in {".rs", ".py", ".ps1"}:
            yield path


def capture(host_source):
    if digest(LEGACY)["lf_sha256"] != LEGACY_SHA256:
        raise RuntimeError("Immutable legacy fixture changed")
    expected = json.loads(LEGACY.read_text(encoding="utf-8-sig"))
    inputs = {}
    for record in expected["records"]:
        path = ROOT / ".artifacts/native-ui-markdown-inputs" / record["fixture"]
        raw = path.read_bytes()
        if raw != record["markdown"].encode("utf-8"):
            raise RuntimeError("Immutable corpus input changed: " + record["fixture"])
        inputs[record["fixture"]] = hashlib.sha256(raw).hexdigest()
    compiled_sources = {str(path.relative_to(ROOT)).replace("\\", "/"): digest(path)
                        for path in sorted(set(sources()))}
    snapshot = {}
    if host_source:
        for path in sorted(host_source.rglob("*")):
            if path.is_file() and (path.suffix == ".rs" or path.name in
                                  {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml"}):
                snapshot[str(path.relative_to(host_source)).replace("\\", "/")] = digest(path)
        for name in ("mod.rs", "inline.rs"):
            if snapshot["src/markdown/" + name]["sha256"] != compiled_sources["mcw/src/markdown/" + name]["sha256"]:
                raise RuntimeError("Compiler snapshot parser bytes differ: " + name)
    return {"captured_utc": datetime.now(timezone.utc).isoformat(),
            "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
            "compiler_sources": compiled_sources, "host_snapshot": snapshot,
            "legacy_fixture": digest(LEGACY), "corpus_inputs": inputs,
            "retained_native_input": {"ThirdParty/WabiSabi/c/build-win/libwabisabi.dll":
                digest(ROOT / "ThirdParty/WabiSabi/c/build-win/libwabisabi.dll")["sha256"]}
                if (ROOT / "ThirdParty/WabiSabi/c/build-win/libwabisabi.dll").is_file() else {},
            "production_integrated": False, "dependency_removed": False,
            "native_in_progress_cancellation_verified": False}


def bind_commit(record, commit):
    tree = subprocess.check_output(["git", "ls-tree", "-r", commit], cwd=ROOT, text=True)
    entries = {}
    for line in tree.splitlines():
        metadata, path = line.split("\t", 1)
        entries[path] = metadata.split()[2]
    failures = [path for path, value in record["compiler_sources"].items()
                if entries.get(path) != value["git_blob"]]
    if entries.get("mcw/tests/native_ui_markdown_legacy.json") != record["legacy_fixture"]["git_blob"]:
        failures.append("mcw/tests/native_ui_markdown_legacy.json")
    if failures:
        raise RuntimeError("Verified source differs from publication: " + str(failures))
    record["bound_publication_commit"] = commit
    record["publication_source_matches"] = True


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host-source", type=Path)
    parser.add_argument("--write", type=Path)
    parser.add_argument("--check", type=Path)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--commit")
    args = parser.parse_args()
    if args.commit:
        if not args.check or not args.report:
            parser.error("Publication binding requires --check and --report")
        record = json.loads(args.check.read_text(encoding="utf-8-sig"))
        bind_commit(record, args.commit)
        destination = args.report
    else:
        record = capture(args.host_source)
        destination = args.write or args.report
        if args.check:
            before = json.loads(args.check.read_text(encoding="utf-8-sig"))
            for key in ("compiler_sources", "host_snapshot", "legacy_fixture", "corpus_inputs", "retained_native_input"):
                if before[key] != record[key]:
                    raise RuntimeError("Compiler inputs changed during verification: " + key)
            record["before_after_inputs_match"] = True
        if not destination:
            parser.error("--write or --report required")
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"report": str(destination), "compiler_sources": len(record["compiler_sources"]),
                      "corpus_inputs": len(record["corpus_inputs"]),
                      "host_snapshot_parser_bytes_match": bool(record["host_snapshot"]),
                      "before_after_inputs_match": record.get("before_after_inputs_match", False),
                      "publication_source_matches": record.get("publication_source_matches", False)}))
