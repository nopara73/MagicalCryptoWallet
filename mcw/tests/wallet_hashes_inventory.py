"""Read tracked source only; record retained callers, package references and audit.

This development script never reads wallet files, credentials, keys or live state.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ref", default="origin/master")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]

    def git(*arguments):
        return subprocess.check_output(["git", "-C", str(root), *arguments])

    commit = git("rev-parse", args.ref).decode().strip()
    paths = git("ls-tree", "-r", "--name-only", commit).decode().splitlines()
    mapped = {
        "MagicalCryptoWallet/Crypto/OwnershipIdentifier.cs": "HMAC-SHA256 over exact script bytes with 32-byte identification key; full MAC comparison should replace SequenceEqual in integration",
        "MagicalCryptoWallet/Crypto/Slip21Node.cs": "HMAC-SHA512 seed and 0x00-prefixed label bytes; retained Key return/type ownership stays managed",
        "MagicalCryptoWallet/Wallets/ShamirSecretSharing/Slip39.cs": "HMAC-SHA256 share digest truncated to 4 bytes; PBKDF2-HMAC-SHA256 Feistel rounds over step-prefixed UTF-8 passphrase and extension-dependent salt",
        "MagicalCryptoWallet/Tor/Control/TorControlClientFactory.cs": "System HMACSHA256 over SAFECOOKIE transcript; transport/handshake/logging policies remain with their owners",
        "MagicalCryptoWallet/Crypto/ProofBody.cs": "SHA256 via actual bitcoin_encoding sibling; retained serialization/signature handling stays managed",
        "MagicalCryptoWallet/Blockchain/Keys/KeyManager.cs": "NBitcoin Mnemonic/ExtKey callers; SHA512/HMAC/PBKDF2 primitives do not replace curve derivation, NFKD or mnemonic ownership",
        "MagicalCryptoWallet.Tests/UnitTests/Crypto/OwnershipProofTest.cs": "Existing public SLIP19 identifier vectors remain managed test references",
        "MagicalCryptoWallet.Tests/UnitTests/Crypto/Slip21NodeTests.cs": "Existing public SLIP21 expected node slices remain managed test references",
    }
    retained = []
    pattern = re.compile(r"HMACSHA(?:256|512)|Rfc2898DeriveBytes\.Pbkdf2|Hashes\.(?:Hash160|RIPEMD160|SHA256|DoubleSHA256)|new Mnemonic|Mnemonic mnemonic|SequenceEqual\(Bytes\)")
    for path, role in mapped.items():
        if path not in paths:
            continue
        body = git("show", f"{commit}:{path}")
        hits = [{"line": number, "source": line.strip()}
                for number, line in enumerate(body.decode().splitlines(), 1) if pattern.search(line)]
        retained.append({"path": path, "role": role, "sha256_git_blob": hashlib.sha256(body).hexdigest(), "hits": hits})
    package_refs = []
    for path in paths:
        if path == "Directory.Packages.props" or path.endswith(".csproj"):
            body = git("show", f"{commit}:{path}").decode()
            for number, line in enumerate(body.splitlines(), 1):
                if re.search(r'<(?:PackageVersion|PackageReference)\b[^>]*Include="NBitcoin(?:\.Secp256k1)?"', line):
                    package_refs.append({"path": path, "line": number, "source": line.strip()})
    direct = git("grep", "-I", "-n", "-E", "HMACSHA|Rfc2898DeriveBytes|RIPEMD|SHA512|Hashes\\.Hash160|Hashes\\.SHA256|Hashes\\.DoubleSHA256",
                 commit, "--", "MagicalCryptoWallet/**/*.cs", "MagicalCryptoWallet.Tests/**/*.cs").decode().splitlines()
    module = root / "mcw/src/wallet_hashes.rs"
    source = module.read_text(encoding="utf-8")
    forbidden = [r"\bextern\s+crate\b", r'\bextern\s+"', r"\bunsafe\s*\{", r"std::process", r"std::net",
                 r"std::fs", r"#\[link", r"include_bytes!", r"include!"]
    for rule in forbidden:
        if re.search(rule, source):
            raise AssertionError("Forbidden runtime dependency or unsafe/source inclusion")
    if "#![forbid(unsafe_code)]" not in source:
        raise AssertionError("Unsafe code must be forbidden")
    imports = re.findall(r"^use ([^;]+);", source, re.M)
    if imports != ["crate::bitcoin_encoding::Sha256", "std::{fmt, hint::black_box}"]:
        raise AssertionError("Review changed module implementation dependencies")
    output = {"schema_version": 1, "audited_commit": commit, "retained_callers": retained,
              "direct_hash_call_sites": direct, "retained_package_references": package_refs,
              "rust_implementation_dependencies": ["Rust stdlib", "first-party bitcoin_encoding::Sha256"],
              "module_sha256_lf": hashlib.sha256(source.encode()).hexdigest(), "unsafe": "forbidden",
              "external_cargo_dependencies": 0, "bundled_runtime_or_library_or_companion": 0,
              "platform_crypto_assumptions": 0, "whole_packages_removed": [],
              "remaining": ["NBitcoin 10.0.13", "NBitcoin.Secp256k1 3.1.6", "managed System.Security.Cryptography hash/PBKDF2 callers",
                            "transitive managed packages", "mcw registration/adapters/secret-bearing bridge policy", "five-target application acceptance"],
              "note": "Read-only source audit. No caller, package, shared manifest, key state or wallet file is migrated."}
    path = root / "mcw/tests/wallet_hashes_callers.json"
    path.write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps({"audited_commit": commit, "retained_callers": len(retained),
                      "direct_hash_call_sites": len(direct), "package_references": len(package_refs),
                      "external_cargo_dependencies": 0, "whole_packages_removed": []}))


if __name__ == "__main__":
    main()
