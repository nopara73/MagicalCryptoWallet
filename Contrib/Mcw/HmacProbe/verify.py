"""Execute the test-only child through the actual Rust host; never shipped."""
from pathlib import Path
import hashlib
import json
import subprocess
import sys


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


root, rust_root, host_source, run, evidence, staged = sys.argv[1:]
root, rust_root, host_source, run, evidence = map(
    Path, (root, rust_root, host_source, run, evidence)
)
fixture = root / "mcw/tests/wallet_hmac_fixtures/independent.tsv"
manifest = json.loads((fixture.parent / "manifest.json").read_text())
assert sha256(fixture) == manifest["fixture_sha256"], "Fixture checksum changed"
report = evidence / "caller-results.json"
if report.exists():
    report.unlink()
result = subprocess.run(
    [str(run / "mcw.exe"), "daemon", str(fixture), str(report)],
    capture_output=True,
    timeout=90,
)
(evidence / "caller-stdout.bin").write_bytes(result.stdout)
(evidence / "caller-stderr.txt").write_bytes(result.stderr)
assert result.returncode == 0, (
    "Actual-caller test failed",
    result.returncode,
    result.stderr.decode(errors="replace")[-2000:],
)
assert not result.stdout, "Caller/host printed to stdout"
for marker in (b"PRIVATE", b"REQUEST", b"RESPONSE"):
    assert b"SYNTHETIC_" + marker + b"_MARKER" not in result.stderr
results = json.loads(report.read_text())
expected = {
    "fixtureCases": 441,
    "actualDomainCalls": 462,
    "asciiLabels": 6,
    "malformedRejected": 2,
    "adapterFailureChecks": 6,
    "callerBoundaryChecks": 3,
    "transportFaultChecks": 37,
}
for key, value in expected.items():
    assert results[key] == value, (key, results[key])
assert results["publicVectors"] and results["connectionSurvived"]
assert results["canceledRequests"] >= 1
hashes = {
    "rust/" + path.relative_to(rust_root).as_posix(): sha256(path)
    for path in sorted(rust_root.rglob("*.rs"))
}
for relative in (
    "MagicalCryptoWallet/Crypto/OwnershipIdentifier.cs",
    "MagicalCryptoWallet/Crypto/Slip21Node.cs",
    "MagicalCryptoWallet/Mcw/Crypto/WalletHmac.cs",
    "Contrib/Mcw/HmacProbe/Program.cs",
    "Contrib/Mcw/HmacProbe/TransportFaults.cs",
    "Contrib/Mcw/HmacProbe/HmacProbe.csproj.inc",
    "Contrib/Mcw/HmacProbe/packages.lock.json.inc",
    "Contrib/Mcw/HmacProbe/verify.ps1",
    "Contrib/Mcw/HmacProbe/verify.py",
):
    hashes[relative] = sha256(root / relative)
hashes["ManagedApplicationHost.cs"] = sha256(host_source)
hashes["generated/HmacProbe.csproj"] = sha256(evidence / "project/HmacProbe.csproj")
hashes["generated/packages.lock.json"] = sha256(evidence / "project/packages.lock.json")
for name in ("mcw.exe", "HmacProbe.dll", "MagicalCryptoWallet.dll"):
    hashes["run/" + name] = sha256(run / name)
results.update(
    source_and_binary_hashes=hashes,
    fixture_sha256=sha256(fixture),
    staged_source_overrides=staged == "true",
    production_incorporation=False,
    targets_executed=["windows-x64"],
    no_stdout=True,
    no_marker_in_stderr=True,
    test_only=True,
)
(evidence / "caller-verification.json").write_text(
    json.dumps(results, indent=2) + "\n", encoding="utf-8", newline="\n"
)
print(json.dumps({k: v for k, v in results.items() if k != "source_and_binary_hashes"}))
