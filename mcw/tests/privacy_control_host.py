#!/usr/bin/env python3
"""Verify Tor control readers through the real mcw/managed application bridge.

Use an isolated checkout with the handoff's exact host and caller patches applied.
The managed child, inputs and TCP server are synthetic; no real Tor/wallet state.
Only the supplied mcw is copied, no extra shipping Rust executable is produced.
The caller/dispatch guards prevent a false pass through a retained managed parser.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--configuration", default="Release", choices=("Debug", "Release"))
    parser.add_argument("--resources", action="store_true", help="Run the near-cap fragmentation/resource/cancellation profile")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    reader = (ROOT / "MagicalCryptoWallet/Tor/Control/TorControlReplyReader.cs").read_text()
    line = (ROOT / "MagicalCryptoWallet/Tor/Control/PipeReaderLineReaderExtension.cs").read_text()
    dispatch = (ROOT / "mcw/src/app.rs").read_text()
    adapter = (ROOT / "MagicalCryptoWallet/Mcw/Privacy/McwTorControlCodec.cs").read_text()
    assert "McwTorControlCodec.ReadReplyAsync" in reader, "Wallet reader has not switched to Rust"
    assert "McwTorControlCodec.ReadLineAsync" in line, "Wallet line reader has not switched to Rust"
    assert "tor_control::service::dispatch" in dispatch, "Production host lacks the codec dispatcher"
    assert "tor_control::service::ChildScope::new()" in dispatch, "Production host lacks child-lifetime decoder cleanup"
    for operation in ("BEGIN", "FEED", "CLOSE"):
        assert "tor_control::service::" + operation in dispatch, "Production host lacks incremental codec operation " + operation
    assert "BeginOperation = 0x0f02" in adapter and "FeedOperation = 0x0f03" in adapter and "CloseOperation = 0x0f04" in adapter, "Managed reader is not the incremental adapter"
    project = ROOT / "Contrib/McwMigration/PrivacyControlProbe"
    subprocess.run(["dotnet", "build", str(project), "-c", args.configuration, "-m:1"], check=True)
    source = project / "bin" / args.configuration / "net10.0"
    evidence = ROOT / ".artifacts/privacy-control-verification"
    evidence.mkdir(parents=True, exist_ok=True)
    suffix = ".exe" if os.name == "nt" else ""
    results = {"host_sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
    with tempfile.TemporaryDirectory(prefix="privacy control ä½ å¥½ ", dir=ROOT / ".artifacts") as temporary:
        work = Path(temporary)
        shutil.copytree(source, work, dirs_exist_ok=True)
        host = work / ("mcw" + suffix)
        shutil.copy2(binary, host)
        for name in ("magicalcryptowallet",):
            shutil.copy2(work / ("PrivacyControlProbe" + suffix), work / (name + suffix))
        for mode in ("gui",):
            report = work / (mode + ".json")
            command = [str(host), mode, str(report), str(ROOT / "mcw/tests/privacy_control_fixtures/replies.tsv")]
            if args.resources:
                command.append("resources")
            result = subprocess.run(command, capture_output=True, timeout=150)
            if report.exists():
                results[mode] = json.loads(report.read_text())
                (evidence / ("resource-results.json" if args.resources else "host-results.json")).write_text(json.dumps(results, indent=2) + "\n")
            assert result.returncode == 0, (mode, result.returncode, result.stderr.decode(errors="replace")[-4000:])
            if args.resources:
                assert results[mode]["failure"] is None
            else:
                assert results[mode]["productionRustBridge"]
            assert not result.stdout, "Tor control payload unexpectedly appeared on host stdout"
    (evidence / ("resource-results.json" if args.resources else "host-results.json")).write_text(json.dumps(results, indent=2) + "\n")
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
