"""Actual mcw/ManagedApplicationHost/RoundState synthetic caller test, no wallets.

Run after building RoundHashProbe and the registered mcw host. Uses only Python
stdlib; the probe is a verification tool and never enters a shipping package.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

root = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument("--binary", type=Path, required=True)
parser.add_argument("--output", type=Path)
options = parser.parse_args()
binary = options.binary.resolve(strict=True)
probe = root / "Contrib/Mcw/RoundHashProbe/bin/Release/net10.0"
fixture = root / "mcw/tests/round_hash_vectors/managed.tsv"
output = (options.output or root / ".artifacts/round-hash-evidence" / str(time.time_ns())).resolve()
output.mkdir(parents=True, exist_ok=False)
child = output / "synthetic child 空白 path"
child.mkdir()
for item in probe.iterdir():
    if item.is_file():
        shutil.copy2(item, child / item.name)
suffix = ".exe" if os.name == "nt" else ""
shutil.copy2(probe / f"RoundHashProbe{suffix}", child / f"magicalcryptowallet{suffix}")
host_binary = child / f"mcw{suffix}"
shutil.copy2(binary, host_binary)
environment = os.environ.copy()
environment.pop("MCW_HOSTED", None)
environment.pop("MCW_HOST_PATH", None)
environment["MAGICALCRYPTOWALLET_DATADIR"] = str(output / "unused synthetic datadir")
checks = [
    ("unavailable", ["dotnet", str(child / "RoundHashProbe.dll"), "unavailable", str(output / "unavailable.json")]),
    ("host", [str(host_binary), "gui", "host", str(output / "host.json"), str(fixture)]),
]
for name, command in checks:
    run = subprocess.run(command, cwd=child, env=environment, capture_output=True, timeout=60)
    (output / f"{name}.stdout").write_bytes(run.stdout)
    (output / f"{name}.stderr").write_bytes(run.stderr)
    if run.returncode:
        raise SystemExit(f"{name} failed with exit {run.returncode}; diagnostics at {output}")
    assert not run.stdout, "The host wrote data outside its private bridge."
    forbidden = ["Synthetic Round Hash", "公開", "MixedCASE", "30313233", "unpaired"]
    for marker in forbidden:
        assert marker.encode() not in run.stdout + run.stderr, "Round metadata escaped to logs."
    assert json.loads((output / f"{name}.json").read_text()), "Probe did not persist its verification."
summary = {"host_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
           "fixture_sha256": hashlib.sha256(fixture.read_bytes().replace(b"\r\n", b"\n")).hexdigest(),
           "host": json.loads((output / "host.json").read_text()),
           "unavailable": json.loads((output / "unavailable.json").read_text()),
           "wallets": 0, "live_coordinator": False, "evidence": str(output)}
(output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary))
