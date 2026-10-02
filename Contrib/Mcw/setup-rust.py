#!/usr/bin/env python3
"""Provision pinned Rust in the build workspace without changing the user's PATH."""
import argparse, hashlib, os, platform, subprocess, urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--rid", required=True)
args = parser.parse_args()
hosts = {"win-x64":"x86_64-pc-windows-msvc","linux-x64":"x86_64-unknown-linux-gnu",
         "linux-arm64":"aarch64-unknown-linux-gnu","osx-x64":"x86_64-apple-darwin","osx-arm64":"aarch64-apple-darwin"}
host = hosts[args.rid]
tools = ROOT / ".artifacts/mcw-toolchain"
tools.mkdir(parents=True, exist_ok=True)
extension = ".exe" if os.name == "nt" else ""
url = f"https://static.rust-lang.org/rustup/dist/{host}/rustup-init{extension}"
installer = tools / ("rustup-init" + extension)
with urllib.request.urlopen(url + ".sha256") as response:
    expected = response.read().decode().split()[0]
if not installer.exists() or hashlib.sha256(installer.read_bytes()).hexdigest() != expected:
    with urllib.request.urlopen(url) as response:
        installer.write_bytes(response.read())
if hashlib.sha256(installer.read_bytes()).hexdigest() != expected: raise RuntimeError("Rustup checksum mismatch")
installer.chmod(0o755)
env = os.environ.copy()
env["RUSTUP_HOME"] = str(tools / "rustup")
env["CARGO_HOME"] = str(tools / "cargo")
subprocess.run([str(installer), "-y", "--no-modify-path", "--profile", "minimal", "--default-toolchain", "1.99.0",
                "--component", "rust-src,rustfmt,clippy"], env=env, check=True)
if filename := os.environ.get("GITHUB_ENV"):
    with open(filename, "a") as output:
        for name in ("RUSTUP_HOME", "CARGO_HOME"): output.write(f"{name}={env[name]}\n")
        output.write(f"CARGO={tools / 'cargo/bin' / ('cargo' + extension)}\n")
if filename := os.environ.get("GITHUB_PATH"):
    with open(filename, "a") as output: output.write(str(tools / "cargo/bin") + "\n")
print("Pinned Rust toolchain: " + str(tools))
print("RUSTUP_HOME=" + env["RUSTUP_HOME"])
print("CARGO_HOME=" + env["CARGO_HOME"])
print("CARGO=" + str(tools / "cargo/bin" / ("cargo" + extension)))
