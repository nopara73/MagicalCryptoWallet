#!/usr/bin/env python3
"""Sign package checksums in an isolated GPG home; never publish anything."""
import hashlib, json, os, subprocess, tempfile, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PACKAGES = Path(sys.argv[1]).resolve() if len(sys.argv) == 2 else ROOT / "packages"
KEYS = json.loads((ROOT / "Contrib/Signing/public-keys.json").read_text())

def run(*args, input=None, capture=False, environment=None):
    return subprocess.run([str(a) for a in args], check=True, input=input, text=True,
                          capture_output=capture, env=environment)

def main():
    for name in ("GPG_PRIVATE_KEY", "GPG_PASSPHRASE", "UPDATE_SIGNING_KEY"):
        if not os.environ.get("MAGICALCRYPTOWALLET_" + name): raise RuntimeError("Missing signing setting: " + name)
    files = sorted(p for p in PACKAGES.iterdir() if p.is_file() and p.name.startswith("MagicalCryptoWallet-") and not p.name.endswith(".asc"))
    if not files: raise RuntimeError("No packages to sign")
    manifest = PACKAGES / "SHA256SUMS"
    manifest.write_text("".join(f"{hashlib.sha256(p.read_bytes()).hexdigest()}  ./{p.name}\n" for p in files), encoding="utf-8")
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
        run("gpg", "--verify", PACKAGES / "SHA256SUMS.asc", environment=environment, capture=True)
        run("gpgconf", "--kill", "gpg-agent", environment=environment)
    project = ROOT / "Contrib/Releases/Publisher/MagicalCryptoWallet.ReleaseTools.csproj"
    run("dotnet", "run", "--project", project, "-c", "Release", "--", "sign-manifest", PACKAGES / "SHA256SUMS.asc", PACKAGES / "SHA256SUMS.magicalcryptowalletsig")
    run("dotnet", "run", "--project", project, "-c", "Release", "--", "verify-manifest", PACKAGES / "SHA256SUMS.asc", PACKAGES / "SHA256SUMS.magicalcryptowalletsig", KEYS["update_public_key"])

if __name__ == "__main__": main()
