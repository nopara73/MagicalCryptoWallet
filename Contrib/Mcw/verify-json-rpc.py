#!/usr/bin/env python3
"""Developer verification: stage the published host plus the exact RPC patch.

Shipping code uses only Rust stdlib and the existing managed domain boundary.
Run under the repository build-slot lock, with CARGO_BUILD_JOBS=1.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


def run(*args, cwd=None):
    subprocess.run([str(arg) for arg in args], cwd=cwd, check=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cargo", default="cargo")
    parser.add_argument("--dotnet", default="dotnet")
    parser.add_argument("--output", type=Path)
    options = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    output = (options.output or root / ".artifacts/rpc-verification").resolve()
    output.relative_to(root)
    output.mkdir(parents=True, exist_ok=True)
    stage = output / "host-stage"
    stage.mkdir(exist_ok=True)
    shutil.copytree(root / "mcw", stage / "mcw", ignore=shutil.ignore_patterns("target"), dirs_exist_ok=True)
    run("git", "init", "-q", cwd=stage)
    patch = root / "Contrib/McwMigration/Patches/json-rpc-host.patch"
    run("git", "apply", "--check", patch, cwd=stage)
    run("git", "apply", patch, cwd=stage)
    manifest = stage / "mcw/Cargo.toml"
    run(options.cargo, "fmt", "--manifest-path", manifest, "--check")
    run(options.cargo, "test", "--manifest-path", manifest, "--offline", "--locked", "--test", "json_rpc_contract")
    run(options.cargo, "clippy", "--manifest-path", manifest, "--offline", "--locked", "--", "-D", "warnings")
    run(options.cargo, "build", "--manifest-path", manifest, "--offline", "--locked")
    # The caller switch is intentionally delivered as an atomic integration
    # patch. Build a disposable source candidate; never mutate the live checkout.
    candidate = output / "managed-candidate"
    candidate.mkdir(exist_ok=True)
    tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=root).decode().split("\0")
    extra = [*root.joinpath("Contrib/Mcw/RpcProbe").glob("*"),
             *root.joinpath("MagicalCryptoWallet/Mcw/Serialization").glob("*.cs"),
             root / "MagicalCryptoWallet.Tests/Helpers/NativeRpcTest.cs"]
    for relative in tracked:
        if not relative:
            continue
        source = root / relative
        if source.is_file():
            destination = candidate / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
    for source in extra:
        if source.is_file():
            destination = candidate / source.relative_to(root)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
    # Required initialized submodule source, excluding all prior build products.
    shutil.copytree(root / "ThirdParty/WabiSabi", candidate / "ThirdParty/WabiSabi",
                    ignore=shutil.ignore_patterns(".git", "bin", "obj"), dirs_exist_ok=True)
    run("git", "init", "-q", cwd=candidate)
    caller_patch = root / "Contrib/McwMigration/Patches/json-rpc-callers.patch"
    check = subprocess.run(["git", "apply", "--check", str(caller_patch)], cwd=candidate, capture_output=True)
    if check.returncode == 0:
        run("git", "apply", caller_patch, cwd=candidate)
    else:
        # Before publication the worker may already have these exact proposed
        # hunks applied. Accept only the identical reverse-applicable state.
        run("git", "apply", "--reverse", "--check", caller_patch, cwd=candidate)
    project = candidate / "Contrib/Mcw/RpcProbe/RpcProbe.csproj"
    run(options.dotnet, "build", project, "-m:1", "--disable-build-servers", "-p:NuGetAudit=false", "-p:BuildMcwHost=false", "--nologo", "-v:q")
    binary = output / "app"
    shutil.copytree(project.parent / "bin/Debug/net10.0", binary, dirs_exist_ok=True)
    suffix = ".exe" if os.name == "nt" else ""
    shutil.copyfile(binary / ("RpcProbe" + suffix), binary / ("magicalcryptowalletd" + suffix))
    target = Path(os.environ.get("CARGO_TARGET_DIR", stage / "mcw/target"))
    executable = binary / ("mcw" + suffix)
    shutil.copyfile(target / "debug" / ("mcw" + suffix), executable)
    shutil.copyfile(executable, project.parent / "bin/Debug/net10.0" / ("mcw" + suffix))
    if os.name != "nt":
        executable.chmod(0o755)
        (binary / "magicalcryptowalletd").chmod(0o755)
    report = output / "rpc-host-report.json"
    run(executable, "daemon", project.parent / "expected.tsv", report, cwd=candidate)
    verified = json.loads(report.read_text())
    if verified["failures"]:
        raise RuntimeError("RPC host verification failed")
    files = [*sorted((root / "mcw/src/serialization_service").glob("*.rs")),
             *sorted((root / "MagicalCryptoWallet/Mcw/Serialization").glob("*.cs")),
             *sorted((root / "MagicalCryptoWallet/Rpc").glob("JsonRpc*.cs")), patch, caller_patch,
             root / "MagicalCryptoWallet.Tests/Helpers/NativeRpcTest.cs",
             root / "Contrib/Mcw/RpcProbe/Program.cs",
             root / "Contrib/Mcw/RpcProbe/RpcProbe.csproj",
             root / "Contrib/Mcw/RpcProbe/SyntheticRpc.cs",
             root / "Contrib/Mcw/RpcProbe/expected.tsv"]
    evidence = {
        "source": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "platform": os.name,
        "hostPatchSha256": hashlib.sha256(patch.read_bytes()).hexdigest(),
        "nativeBinarySha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
        "sources": {str(file.relative_to(root)): hashlib.sha256(file.read_bytes()).hexdigest() for file in files},
        "candidateSources": {relative: hashlib.sha256((candidate / relative).read_bytes()).hexdigest()
                             for relative in ["MagicalCryptoWallet/Rpc/JsonRpcRequest.cs",
                                              "MagicalCryptoWallet/Rpc/JsonRpcResponse.cs",
                                              "MagicalCryptoWallet/Rpc/JsonRpcRequestHandler.cs",
                                              "MagicalCryptoWallet/Rpc/JsonRpcServer.cs"]},
        "result": verified,
        "otherTargets": "not verified by this run",
    }
    (output / "rpc-verification.json").write_text(json.dumps(evidence, indent=2) + "\n")
    print(json.dumps({"verified": True, "fixtures": verified["fixtures"], "report": str(report)}))


if __name__ == "__main__":
    main()
