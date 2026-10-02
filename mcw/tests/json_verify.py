"""Repeatable first-party JSON component verification; Python standard library only.

rustc/rustfmt are developer tools, not shipping dependencies. Optional cached
Newtonsoft/.NET is an independent test oracle in an ignored temporary project.
Nothing touches wallet files, peer sources, manifests, the index, or toolchains.
Take a shared build slot before enabling the managed oracle, per coordination.
"""

from __future__ import annotations

import argparse
from decimal import Decimal, localcontext
import hashlib
import json
import os
from pathlib import Path
import random
import re
import subprocess
import sys
import time
from xml.sax.saxutils import escape

TESTS = Path(__file__).resolve().parent
ROOT = TESTS.parent.parent
FIXTURES = TESTS / "json_vectors"
TARGETS = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
           "x86_64-apple-darwin", "aarch64-apple-darwin"]


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def checked(command: list[str], output: Path | None = None, *, stdin: str | None = None) -> str:
    result = subprocess.run(command, cwd=ROOT, input=stdin, capture_output=True,
                            encoding="utf-8", errors="strict", check=False)
    text = result.stdout + result.stderr
    if output:
        output.write_text(text, encoding="utf-8", newline="\n")
    if result.returncode:
        raise RuntimeError(f"exit {result.returncode}: {command[0]}\n{text}")
    return result.stdout


def fingerprint() -> dict[str, str]:
    files = [ROOT / "mcw/src/json.rs", *sorted((ROOT / "mcw/src/json").rglob("*.rs")),
             TESTS / "json_conformance.rs", TESTS / "json_managed_reference.cs",
             TESTS / "json_verify.py", TESTS / "json_vectors.sha256"]
    return {str(p.relative_to(ROOT)).replace("\\", "/"): digest(p) for p in files}


def verify_fixture_hashes() -> int:
    expected = {}
    for line in (TESTS / "json_vectors.sha256").read_text(encoding="utf-8").splitlines():
        sha, relative = line.split("  ", 1)
        path = TESTS / relative
        if not path.resolve().is_relative_to(FIXTURES.resolve()):
            raise ValueError("fixture manifest escapes owned fixtures")
        if digest(path) != sha:
            raise ValueError(f"fixture bytes changed: {relative}")
        expected[relative] = sha
    actual = {str(p.relative_to(TESTS)).replace("\\", "/") for p in FIXTURES.rglob("*") if p.is_file()}
    if actual != set(expected):
        raise ValueError("fixture manifest does not include exactly the fixture tree")
    return len(expected)


def equal_semantics(a, b) -> bool:
    if isinstance(a, bool) or isinstance(b, bool):
        return type(a) is type(b) and a == b
    if isinstance(a, (Decimal, int)) and isinstance(b, (Decimal, int)):
        return a == b
    if type(a) is not type(b):
        return False
    if isinstance(a, dict):
        return list(a) == list(b) and all(equal_semantics(a[key], b[key]) for key in a)
    if isinstance(a, list):
        return len(a) == len(b) and all(equal_semantics(x, y) for x, y in zip(a, b))
    return a == b


def reference_load(path: Path):
    return json.loads(path.read_text(encoding="utf-8"), parse_float=Decimal, parse_int=int)


def probe_source() -> str:
    module = json.dumps(str(ROOT / "mcw/src/json.rs"))
    return f'''#[path = {module}]
#[allow(dead_code)]
mod json;
use std::io::BufRead;
fn main() {{
    let args: Vec<String> = std::env::args().collect();
    if args[1] == "application" {{
        std::fs::create_dir_all(&args[3]).unwrap();
        for entry in std::fs::read_dir(&args[2]).unwrap() {{
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {{ continue; }}
            let input = std::fs::read(&path).unwrap();
            let document = json::parse(&input, &json::ParseOptions::legacy_config()).unwrap();
            let encoded = json::serialize(document.root(), &json::SerializeOptions::default()).unwrap();
            std::fs::write(std::path::Path::new(&args[3]).join(path.file_name().unwrap()), encoded).unwrap();
        }}
    }} else if args[1] == "numbers" {{
        for line in std::io::stdin().lock().lines() {{
            let line = line.unwrap();
            let (text, scale) = line.split_once('\\t').unwrap();
            match json::Number::parse(text).unwrap().to_scaled_i128(scale.parse().unwrap()) {{
                Ok(units) => println!("ok\\t{{units}}"),
                Err(error) => println!("err\\t{{error:?}}"),
            }}
        }}
    }} else {{ panic!("unknown verification mode"); }}
}}
'''


def numeric_differential(probe: Path, output: Path) -> int:
    rng = random.Random(0x77EC09DA508A353B)
    cases = [(str(n), scale) for n in [0, 1, -1, 2**53 + 1, 2**63 - 1, -2**63,
             2**64 - 1, 2**127 - 1, -2**127, 2**127, -2**127 - 1] for scale in [0, 1, 8, 38, 160]]
    for _ in range(10_000):
        integer = rng.choice(["0", str(rng.randrange(1, 10**rng.randrange(1, 100)))])
        fraction = "." + "".join(str(rng.randrange(10)) for _ in range(rng.randrange(1, 100))) if rng.randrange(2) else ""
        exponent = f"{rng.choice(['e', 'E'])}{rng.randrange(-400, 401):+d}" if rng.randrange(2) else ""
        text = rng.choice(["", "-"]) + integer + fraction + exponent
        cases.append((text, rng.randrange(0, 161)))
    inputs = "".join(f"{text}\t{scale}\n" for text, scale in cases)
    lines = checked([str(probe), "numbers"], output, stdin=inputs).splitlines()
    if len(lines) != len(cases):
        raise AssertionError("number oracle output count mismatch")
    with localcontext() as context:
        context.prec = 2000
        for (text, scale), result in zip(cases, lines):
            scaled = Decimal(text) * Decimal(10) ** scale
            if scaled != scaled.to_integral_value():
                expected = "err\tNonIntegral"
            elif not -(2**127) <= scaled <= 2**127 - 1:
                expected = "err\tOutOfRange"
            else:
                expected = f"ok\t{int(scaled)}"
            if result != expected:
                raise AssertionError(f"exact decimal differential mismatch: {text}, scale {scale}: {result} != {expected}")
    return len(cases)


def managed_differential(dotnet: str, dll: Path, artifacts: Path, rust_outputs: Path) -> dict:
    project = artifacts / "managed-reference"
    project.mkdir(parents=True, exist_ok=True)
    csproj = project / "json-reference.csproj"
    csproj.write_text(f'''<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><TargetFramework>net10.0</TargetFramework><OutputType>Exe</OutputType>
    <ImplicitUsings>disable</ImplicitUsings><Nullable>enable</Nullable>
    <EnableDefaultCompileItems>false</EnableDefaultCompileItems>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit>
  </PropertyGroup>
  <ItemGroup><Compile Include="{escape(str(TESTS / 'json_managed_reference.cs'))}" />
    <Reference Include="Newtonsoft.Json"><HintPath>{escape(str(dll))}</HintPath></Reference>
  </ItemGroup>
</Project>''', encoding="utf-8")
    checked([dotnet, "build", str(csproj), "--configuration", "Release", "--nologo", "--disable-build-servers", "-v:q", "-m:1",
             "-p:ImportDirectoryBuildProps=false", "-p:ImportDirectoryBuildTargets=false",
             "-p:ManagePackageVersionsCentrally=false", "-p:BuildInParallel=false", "-p:NuGetAudit=false"], artifacts / "managed-build.txt")
    result_dir = artifacts / "managed-output"
    checked([dotnet, str(project / "bin/Release/net10.0/json-reference.dll"), str(FIXTURES / "application"), str(result_dir)], artifacts / "managed-run.txt")
    count = 0
    for path in sorted(rust_outputs.glob("*.json")):
        actual = reference_load(path)
        for engine in ["newtonsoft", "system-text-json"]:
            if not equal_semantics(actual, reference_load(result_dir / engine / path.name)):
                raise AssertionError(f"{engine} application payload semantic/order mismatch: {path.name}")
        count += 1
    if count != 12:
        raise AssertionError("expected exactly twelve synthetic application payloads")
    behaviors = reference_load(result_dir / "behavior.json")
    required = {"default_date_token": "Date", "explicit_date_none_token": "String",
                "duplicate_count": 1, "duplicate_value": 2, "newtonsoft_single_quotes": True,
                "newtonsoft_unquoted_keys": True, "newtonsoft_comments": True,
                "newtonsoft_trailing_comma": True, "managed_comments": True,
                "managed_trailing_comma": False, "managed_trailing_comma_opt_in": True,
                "legacy_double_integer": 9007199254740992, "exact_decimal_integer": "9007199254740993"}
    for key, expected in required.items():
        if behaviors[key] != expected:
            raise AssertionError(f"managed compatibility behavior drift: {key}")
    return {"payloads": count, "engines": ["Newtonsoft.Json (Decimal/DateParseHandling.None)", "System.Text.Json (comments skip)"],
            "newtonsoft_sha256": digest(dll), "behaviors": behaviors}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--rustc", required=True)
    parser.add_argument("--rustfmt", required=True)
    parser.add_argument("--dotnet")
    parser.add_argument("--newtonsoft-dll", type=Path)
    args = parser.parse_args()
    if bool(args.dotnet) != bool(args.newtonsoft_dll):
        parser.error("--dotnet and --newtonsoft-dll must be supplied together")
    artifacts = ROOT / ".artifacts/json-verification"
    artifacts.mkdir(parents=True, exist_ok=True)
    os.environ["CARGO_BUILD_JOBS"] = "1"
    started = time.monotonic()
    initial = fingerprint()
    fixture_count = verify_fixture_hashes()
    source_paths = [ROOT / "mcw/src/json.rs", *sorted((ROOT / "mcw/src/json").rglob("*.rs"))]
    for path in source_paths:
        if re.search(r"\bunsafe\b|\bf(?:32|64)\b|extern\s+crate|#\[link", path.read_text(encoding="utf-8")):
            raise AssertionError("JSON module has native/unsafe/external/floating-point implementation code")
    checked([args.rustfmt, "--check", "--edition", "2024", *(str(p) for p in source_paths), str(TESTS / "json_conformance.rs")], artifacts / "format.txt")
    rust_version = checked([args.rustc, "--version"]).strip()
    if not rust_version.startswith("rustc 1.99.0 "):
        raise AssertionError(f"Rust 1.99.0 required, got {rust_version}")
    test_binary = artifacts / ("json-tests.exe" if os.name == "nt" else "json-tests")
    checked([args.rustc, "--edition=2024", "--test", str(TESTS / "json_conformance.rs"), "-D", "warnings", "-o", str(test_binary)], artifacts / "rust-build.txt")
    test_output = checked([str(test_binary), "--test-threads=1", "--nocapture"], artifacts / "rust-tests.txt")
    passed = re.search(r"test result: ok\. (\d+) passed; 0 failed", test_output)
    if not passed:
        raise AssertionError("Rust test success marker missing")
    probe_rs = artifacts / "json-reference-probe.rs"
    probe_rs.write_text(probe_source(), encoding="utf-8", newline="\n")
    probe_binary = artifacts / ("json-reference-probe.exe" if os.name == "nt" else "json-reference-probe")
    checked([args.rustc, "--edition=2024", str(probe_rs), "-D", "warnings", "-o", str(probe_binary)], artifacts / "probe-build.txt")
    rust_outputs = artifacts / "rust-output"
    checked([str(probe_binary), "application", str(FIXTURES / "application"), str(rust_outputs)], artifacts / "application-run.txt")
    numeric_count = numeric_differential(probe_binary, artifacts / "number-differential.txt")
    metadata_rs = artifacts / "json-metadata.rs"
    metadata_rs.write_text(f'#[path = {json.dumps(str(ROOT / "mcw/src/json.rs"))}]\npub mod json;\n', encoding="utf-8")
    sysroot = Path(checked([args.rustc, "--print", "sysroot"]).strip())
    target_results = {}
    for target in TARGETS:
        if not (sysroot / "lib/rustlib" / target / "lib").is_dir():
            target_results[target] = "not installed; target compile/execution unverified"
            continue
        checked([args.rustc, "--edition=2024", "--crate-type=lib", "--emit=metadata", "--target", target,
                 str(metadata_rs), "-D", "warnings", "-o", str(artifacts / f"json-{target}.rmeta")], artifacts / f"metadata-{target}.txt")
        target_results[target] = "metadata/typecheck passed; native execution only for host test target"
    managed = managed_differential(args.dotnet, args.newtonsoft_dll.resolve(), artifacts, rust_outputs) if args.dotnet else {"status": "not run"}
    if fingerprint() != initial:
        raise AssertionError("source or verification inputs changed during verification")
    evidence = {"scope": "JSON component; no production integration or package removal claim",
                "rust": rust_version, "source_sha256": initial, "fixture_files": fixture_count,
                "rust_tests_passed": int(passed.group(1)), "json_testsuite": {"required_accept": 95, "required_reject": 188, "optional_accept": 10, "optional_reject": 25},
                "exact_decimal_differential_cases": numeric_count, "managed_reference": managed,
                "target_metadata": target_results, "module_runtime_dependencies": ["Rust standard library"],
                "elapsed_seconds": round(time.monotonic() - started, 3)}
    (artifacts / "evidence.json").write_text(json.dumps(evidence, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps(evidence, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
