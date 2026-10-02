"""Snapshot actual published host/caller code plus the bounded content leaves.

Shared-owner integration patches apply only in this disposable test snapshot.
No active checkout, wallet, Tor process, or production configuration is changed.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--integrated", action="store_true",
                        help="Test incorporated master without applying any shared patch")
    args = parser.parse_args()
    repo = Path(args.repo).resolve()
    out = Path(args.out).resolve()
    assert out.is_relative_to(repo / ".artifacts"), "Snapshot must be in this task's artifacts"
    assert not out.exists(), "Use a fresh snapshot; preserve earlier evidence"
    out.mkdir(parents=True)
    tracked = subprocess.check_output([
        "git", "ls-files", "-z", "--", "MagicalCryptoWallet", "mcw", "Contrib/Mcw",
        "ThirdParty/WabiSabi/csharp", "ThirdParty/WabiSabi/Directory.Build.props",
        "ThirdParty/WabiSabi/LICENSE", "ThirdParty/WabiSabi/README.md",
        "Directory.Build.props", "Directory.Build.targets", "Directory.Packages.props",
        "global.json", "NuGet.Config", "BannedSymbols.txt", ".editorconfig", ".gitattributes",
        "MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs",
    ], cwd=repo).decode().split("\0")
    files = set(filter(None, tracked))
    # Review-only source belongs below Cargo's auto-discovered integration tests.
    files.discard("mcw/tests/compression_content_inbox_tests.rs")
    files.add("mcw/tests/compression_fixtures/content_inbox_tests.rs")
    files.add("mcw/tests/compression_fixtures/content_raw_host.cs")
    files = {p for p in files if not p.startswith((
        "MagicalCryptoWallet/BundledApps/Binaries/", "MagicalCryptoWallet/Tor/Geoip/"))}
    for directory in ("mcw/src/content_service", "MagicalCryptoWallet/Mcw/Content"):
        files.update(str(p.relative_to(repo)).replace("\\", "/")
                     for p in (repo / directory).rglob("*") if p.is_file())
    files.update("mcw/tests/" + name for name in (
        "compression_content_host_fixture.cs", "compression_content_host_patch.py",
        "compression_content_host_prepare.py", "compression_content_host_run.py",
        "compression_content_host_verify.ps1"))
    # Record every copied source, including the real interface, caller, parser,
    # retry handler, host bridge, native platform/lifetime code and build scripts.
    source_hashes = {}
    for name in sorted(files):
        source = repo / name
        target = out / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
        source_hashes[name] = digest(source)
    patch_record = None
    if args.integrated:
        assert "pub mod content_service;" in (repo / "mcw/src/lib.rs").read_text()
        app = (repo / "mcw/src/app.rs").read_text()
        assert "content_adapter::OPERATION =>" in app or "crate::content_service::adapter::OPERATION =>" in app
        assert "content_adapter::execute(" in app or "crate::content_service::adapter::execute(" in app
        assert "inbox.is_interrupted(frame.id, frame.operation)" in app, "Real host interruption hook required"
        assert "fn is_interrupted(" in (repo / "mcw/src/app/inbox.rs").read_text()
        factory = (repo / "MagicalCryptoWallet/WebClients/MagicalCryptoWallet/MagicalCryptoWalletHttpClientFactory.cs").read_text()
        assert "McwContentDecodingHandler" in factory and "DecompressionMethods.None" in factory
    else:
        patch_dir = out / ".artifacts/shared-patches"
        subprocess.run([sys.executable, str(repo / "mcw/tests/compression_content_host_patch.py"),
                        "--repo", str(repo), "--out", str(patch_dir)], check=True)
        patch_record = json.loads((patch_dir / "patch-manifest.json").read_text())
        for name in patch_record["source_hashes"]:
            shutil.copy2(patch_dir / "review" / name, out / name)

    fixture = out / "ContentActualHost"
    fixture.mkdir()
    project = ET.Element("Project", {"Sdk": "Microsoft.NET.Sdk"})
    props = ET.SubElement(project, "PropertyGroup")
    for name, value in {
        "OutputType": "Exe", "AssemblyName": "McwContentActualHostFixture",
        "PackageId": "McwContentActualHostFixture", "EnableDefaultCompileItems": "false",
        "UseAppHost": "true", "BuildMcwHost": "false", "UseSharedCompilation": "false",
    }.items():
        ET.SubElement(props, name).text = value
    items = ET.SubElement(project, "ItemGroup")
    ET.SubElement(items, "ProjectReference", {"Include": "../MagicalCryptoWallet/MagicalCryptoWallet.csproj"})
    ET.SubElement(items, "Compile", {"Include": "../MagicalCryptoWallet.Client/Application/ManagedApplicationHost.cs"})
    ET.SubElement(items, "Compile", {"Include": "../mcw/tests/compression_content_host_fixture.cs"})
    ET.SubElement(items, "Compile", {"Include": "../mcw/tests/compression_fixtures/content_raw_host.cs"})
    ET.indent(project)
    ET.ElementTree(project).write(fixture / "ContentActualHost.csproj", encoding="unicode")
    record = {
        "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo).decode().strip(),
        "source_root": str(repo), "snapshot_root": str(out), "source_hashes": source_hashes,
        "snapshot_hashes": {name: digest(out / name) for name in sorted(files)},
        "shared_patches": patch_record, "actual_application_host": True,
        "production_integrated": args.integrated, "test_local_shared_patches": not args.integrated,
        "cancellation_scope": "HTTP body acquisition only; no synchronized native decode marker in this fixture",
        "in_flight_native_cancellation_verified": False,
        "synthetic_only": True, "external_network": False, "active_checkouts_modified": False,
        "named_client": "MempoolSpace-bitcoin-fee-rate-provider",
    }
    (out / ".artifacts/source-manifest.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"snapshot": str(out), "copied_files": len(files),
                      "source_commit": record["source_commit"],
                      "test_local_shared_patches": not args.integrated}))


if __name__ == "__main__":
    main()
