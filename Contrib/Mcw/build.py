#!/usr/bin/env python3
"""Build the one shipping mcw executable and verify the empty Cargo graph."""
import argparse, json, os, shutil, subprocess, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TARGETS = {"win-x64": "x86_64-pc-windows-msvc", "linux-x64": "x86_64-unknown-linux-gnu",
           "linux-arm64": "aarch64-unknown-linux-gnu", "osx-x64": "x86_64-apple-darwin",
           "osx-arm64": "aarch64-apple-darwin"}

def build(rid, version="99.99.99", test=False):
    cargo = os.environ.get("CARGO", "cargo")
    env = os.environ.copy()
    env["MCW_VERSION"] = version
    env.setdefault("CARGO_BUILD_JOBS", "1")
    if os.name == "nt" and not env.get("VCTOOLSINSTALLDIR"):
        vswhere = Path(os.environ["ProgramFiles(x86)"]) / "Microsoft Visual Studio/Installer/vswhere.exe"
        vs = subprocess.check_output([str(vswhere), "-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"]).decode().strip()
        script = Path(vs) / "Common7/Tools/Launch-VsDevShell.ps1"
        command = "& '" + str(script).replace("'", "''") + "' -Arch amd64 -HostArch amd64 -SkipAutomaticLocation *> $null; [Environment]::GetEnvironmentVariables('Process') | ConvertTo-Json -Compress"
        # Windows environment names are case-insensitive; Python normalizes its
        # own keys to uppercase, while DevShell's JSON preserves their spelling.
        env.update({key.upper():value for key,value in json.loads(subprocess.check_output(
            ["pwsh", "-NoProfile", "-Command", command]).decode("utf-8-sig")).items()})
        env["CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER"] = shutil.which("link.exe", path=env["PATH"])
        if not env["CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER"]:
            raise RuntimeError("Visual Studio's native linker is unavailable")
    target = TARGETS[rid]
    env.setdefault("CARGO_TARGET_DIR", str(ROOT / ".artifacts/mcw-build"))
    # Reading rust-toolchain.toml must work even when invoked from the repo root.
    cwd = ROOT / "mcw"
    metadata = json.loads(subprocess.check_output([cargo, "metadata", "--offline", "--locked", "--format-version", "1"], cwd=cwd, env=env))
    assert len(metadata["packages"]) == 1 and not metadata["packages"][0]["dependencies"], "External Cargo dependencies are prohibited"
    assert subprocess.check_output([str(Path(cargo).with_name("rustc.exe" if os.name == "nt" else "rustc")), "--version"], cwd=cwd, env=env).decode().startswith("rustc 1.99.0 ")
    if test:
        subprocess.run([cargo, "fmt", "--check"], cwd=cwd, env=env, check=True)
        # Component workers also publish standalone integration-test harnesses.
        # Lint the shipping library/executable strictly; run every functional test.
        subprocess.run([cargo, "clippy", "--lib", "--bin", "mcw", "--locked", "--offline", "--", "-D", "warnings"], cwd=cwd, env=env, check=True)
        subprocess.run([cargo, "test", "--target", target, "--locked", "--offline", "--", "--test-threads=1"], cwd=cwd, env=env, check=True)
    if rid.startswith("win"):
        subprocess.run(["pwsh", "-NoProfile", "-File", str(ROOT / "Contrib/Mcw/build-windows.ps1"), "-Cargo", cargo,
                        "-Version", version, "-PrepareStandardLibrary"], cwd=cwd, env=env, check=True)
    elif rid.startswith("linux"):
        # Linux's prebuilt std includes the GCC unwinder through backtrace support.
        # Rebuild the matching std without backtraces, preserving aborting panics
        # and OS libc while removing libgcc_s rather than bundling/static linking it.
        env["RUSTC_BOOTSTRAP"] = "1"
        env["RUSTFLAGS"] = "-C panic=abort -C default-linker-libraries=no"
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        subprocess.run([cargo, "-Z", "build-std=std,panic_abort", "-Z", "build-std-features=",
                        "build", "--release", "--target", target, "--locked"], cwd=cwd, env=env, check=True)
    else:
        subprocess.run([cargo, "build", "--release", "--target", target, "--locked", "--offline"], cwd=cwd, env=env, check=True)
    binary = Path(env["CARGO_TARGET_DIR"]) / target / "release" / ("mcw.exe" if rid.startswith("win") else "mcw")
    if not binary.is_file(): raise RuntimeError("mcw build output is missing")
    subprocess.run([sys.executable, str(ROOT / "Contrib/Mcw/audit.py"), "--binary", str(binary)], check=True, env=env)
    return binary

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rid", required=True, choices=TARGETS)
    parser.add_argument("--version", default=os.environ.get("MAGICALCRYPTOWALLET_VERSION", "99.99.99"))
    parser.add_argument("--test", action="store_true")
    parser.add_argument("--copy-to", type=Path)
    args = parser.parse_args()
    binary = build(args.rid, args.version, args.test)
    if args.copy_to:
        args.copy_to.mkdir(parents=True, exist_ok=True)
        shutil.copy2(binary, args.copy_to / binary.name)
    print(binary)
