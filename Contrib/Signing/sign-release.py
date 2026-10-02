#!/usr/bin/env python3
"""Sign package checksums in an isolated GPG home; never publish anything."""
import argparse, hashlib, json, os, re, subprocess, tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
KEYS = json.loads((ROOT / "Contrib/Signing/public-keys.json").read_text())
SUFFIXES = json.loads((ROOT / "Contrib/Signing/package-suffixes.json").read_text())
EXTENSIONS = {suffix[suffix.index("."):] for suffix in SUFFIXES}

def select_packages(directory: Path, version: str) -> list[Path]:
    if (not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:\.[0-9]+)?", version)
            or version != ".".join(str(int(part)) for part in version.split("."))
            or any(int(part) > 2147483647 for part in version.split("."))):
        raise RuntimeError("Expected a numeric 3- or 4-part release version")
    expected = {f"MagicalCryptoWallet-{version}{suffix}" for suffix in SUFFIXES}
    files = sorted(p for p in directory.iterdir() if p.is_file()
                   and p.name.startswith("MagicalCryptoWallet-")
                   and any(p.name.endswith(extension) for extension in EXTENSIONS))
    if not files:
        raise RuntimeError("No packages to sign")
    if any(p.name not in expected for p in files):
        raise RuntimeError("Packages contain a different release version or unsupported target")
    return files

def run(*args, input=None, capture=False, environment=None):
    return subprocess.run([str(a) for a in args], check=True, input=input, text=True,
                          capture_output=capture, env=environment)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("packages", type=Path, nargs="?", default=ROOT / "packages")
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    packages = args.packages.resolve()
    files = select_packages(packages, args.version)
    for name in ("GPG_PRIVATE_KEY", "GPG_PASSPHRASE", "UPDATE_SIGNING_KEY"):
        if not os.environ.get("MAGICALCRYPTOWALLET_" + name): raise RuntimeError("Missing signing setting: " + name)
    manifest = packages / "SHA256SUMS"
    checksums = []
    for package in files:
        with package.open("rb") as stream:
            checksums.append(f"{hashlib.file_digest(stream, 'sha256').hexdigest()}  ./{package.name}\n")
    manifest.write_text("".join(checksums), encoding="utf-8")
    with tempfile.TemporaryDirectory(prefix="magicalcryptowallet-gpg-") as temporary:
        environment = os.environ.copy(); environment["GNUPGHOME"] = temporary
        run("gpg", "--batch", "--import", input=environment["MAGICALCRYPTOWALLET_GPG_PRIVATE_KEY"], environment=environment, capture=True)
        fingerprint = KEYS["gpg_fingerprint"]
        listing = run("gpg", "--batch", "--with-colons", "--list-secret-keys", environment=environment, capture=True).stdout
        if "fpr:::::::::" + fingerprint + ":" not in listing: raise RuntimeError("Wrong GPG signing key")
        common = ["gpg", "--batch", "--yes", "--pinentry-mode", "loopback", "--passphrase-fd", "0", "--local-user", fingerprint]
        for package in files:
            run(*common, "--armor", "--detach-sign", package, input=environment["MAGICALCRYPTOWALLET_GPG_PASSPHRASE"] + "\n", environment=environment)
        run(*common, "--digest-algo", "SHA256", "--clearsign", manifest, input=environment["MAGICALCRYPTOWALLET_GPG_PASSPHRASE"] + "\n", environment=environment)
        run("gpg", "--verify", packages / "SHA256SUMS.asc", environment=environment, capture=True)
        run("gpgconf", "--kill", "gpg-agent", environment=environment)
    project = ROOT / "Contrib/Releases/Publisher/MagicalCryptoWallet.ReleaseTools.csproj"
    run("dotnet", "run", "--project", project, "-c", "Release", "--", "sign-manifest", packages / "SHA256SUMS.asc", packages / "SHA256SUMS.magicalcryptowalletsig")
    run("dotnet", "run", "--project", project, "-c", "Release", "--", "verify-manifest", packages / "SHA256SUMS.asc", packages / "SHA256SUMS.magicalcryptowalletsig", KEYS["update_public_key"])

if __name__ == "__main__": main()
