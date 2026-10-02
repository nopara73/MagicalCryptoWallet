"""Run only the owned synthetic child through the actual built native host."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", required=True)
    args = parser.parse_args()
    snapshot = Path(args.snapshot).resolve()
    output = snapshot / "ContentActualHost/bin/Release/net10.0"
    host = output / "mcw.exe"
    assert host.is_file() and (output / "magicalcryptowallet.exe").is_file()
    log = snapshot / ".artifacts/actual-host.log"
    with log.open("wb") as stream:
        # Known, test-owned process only. The actual native Job Object owns its
        # one managed child; neither process starts Tor or touches wallet data.
        result = subprocess.run([str(host)], cwd=output, stdout=stream, stderr=stream,
                                timeout=150, creationflags=subprocess.CREATE_NO_WINDOW)
    text = log.read_text(encoding="utf-8-sig")
    print(text)
    assert result.returncode == 0, f"Actual host exited {result.returncode}"
    assert text.count("ACTUAL CONTENT HOST RESULT: 26 passed; actual_application_host=true; synthetic_only=true") == 1
    manifest = json.loads((snapshot / ".artifacts/source-manifest.json").read_text())
    for name, expected in manifest["source_hashes"].items():
        assert hashlib.sha256((Path(manifest["source_root"]) / name).read_bytes()).hexdigest() == expected, name
    for name, expected in manifest["snapshot_hashes"].items():
        assert hashlib.sha256((snapshot / name).read_bytes()).hexdigest() == expected, name
    record = dict(manifest, actual_host_passed=26, native_exit_code=result.returncode,
                  binary_sha256=hashlib.sha256(host.read_bytes()).hexdigest(),
                  binary_path=str(host), log_path=str(log),
                  cancellation_scope="HTTP body acquisition; this fixture has no synchronized in-flight native marker",
                  in_flight_native_cancellation_verified=False)
    (snapshot / ".artifacts/verification.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
