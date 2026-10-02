"""Independent differential checks, using an ignored temporary rustc harness.

Requires only Python standard library as development tooling, plus the separately
downloaded BIP reference Python file whose content hash is checked below.
Neither the reference implementation nor this harness ships with mcw.
"""
import argparse
import hashlib
import importlib.util
import json
import random
import subprocess
from collections import Counter
from pathlib import Path

REFERENCE_SHA256 = "2884ce04a36c8374c4249177cd90e91e545ac73b5c9260307bbeb74ae2e9de0f"
ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

HARNESS = r'''
#![forbid(unsafe_code)]
#![allow(dead_code)]
#[path = __SOURCE_PATH__]
mod bitcoin_encoding;
use bitcoin_encoding::*;
use std::io::{self, BufRead};

fn network(s: &str) -> Network {
    match s {
        "main" => Network::Mainnet, "test" => Network::Testnet,
        "testnet4" => Network::Testnet4, "signet" => Network::Signet,
        "regtest" => Network::Regtest, _ => panic!("invalid test network")
    }
}
fn result<T>(r: Result<T, Error>, output: impl FnOnce(T) -> String) -> String {
    match r { Ok(x) => output(x), Err(_) => "ERR".to_owned() }
}
fn run(line: &str) -> String {
    let f: Vec<&str> = line.split('\t').collect();
    match f[0] {
        "hex" => result(hex_decode(f[1]), |bytes| hex_encode(&bytes).unwrap()),
        "sha" => {
            let bytes = hex_decode(f[1]).unwrap();
            result(sha256(&bytes), |hash| hex_encode(&hash).unwrap())
        }
        "sha_fragment" => {
            let bytes = hex_decode(f[1]).unwrap();
            let chunk: usize = f[2].parse().unwrap();
            let mut hash = Sha256::new();
            for part in bytes.chunks(chunk) {
                hash.update(&[]).unwrap();
                hash.update(part).unwrap();
            }
            hex_encode(&hash.finalize()).unwrap()
        }
        "sha_double" => {
            let bytes = hex_decode(f[1]).unwrap();
            result(double_sha256(&bytes), |hash| hex_encode(&hash).unwrap())
        }
        "base58" => {
            let bytes = hex_decode(f[1]).unwrap();
            result(base58_encode(&bytes), |text| text)
        }
        "base58_decode" => result(base58_decode(f[1]), |bytes| hex_encode(&bytes).unwrap()),
        "check" => {
            let bytes = hex_decode(f[1]).unwrap();
            result(base58check_encode(&bytes), |text| text)
        }
        "check_decode" => result(base58check_decode(f[1]), |bytes| hex_encode(&bytes).unwrap()),
        "bech" => {
            let bytes = hex_decode(f[3]).unwrap();
            let spec = if f[2] == "1" { ChecksumVariant::Bech32 } else { ChecksumVariant::Bech32m };
            result(bech32_encode(f[1], &bytes, spec), |text| text)
        }
        "bech_decode" => result(bech32_decode(f[1]), |data| format!("{}\t{}\t{}", data.hrp,
            if data.variant == ChecksumVariant::Bech32 { 1 } else { 2 }, hex_encode(&data.data).unwrap())),
        "witness" => {
            let bytes = hex_decode(f[3]).unwrap();
            let version = f[2].parse().unwrap();
            result(witness_address_encode(network(f[1]), version, &bytes), |text| text)
        }
        "witness_decode" => result(witness_address_decode(f[2], network(f[1])), |address|
            format!("{}\t{}", address.version, hex_encode(&address.program).unwrap())),
        "address_decode" => result(address_decode(f[2], network(f[1])), |address|
            address_encode(&address).unwrap()),
        _ => panic!("invalid test operation"),
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        println!("{}", run(&line.unwrap()));
    }
}
'''


def base58_encode(data):
    # Independent arbitrary-precision conversion rather than the Rust digit loop.
    number = int.from_bytes(data, "big")
    digits = ""
    while number:
        number, digit = divmod(number, 58)
        digits = ALPHABET[digit] + digits
    zeros = len(data) - len(data.lstrip(b"\0"))
    return "1" * zeros + digits


def double_hash(data):
    return hashlib.sha256(hashlib.sha256(data).digest()).digest()


def base58check(data):
    return base58_encode(data + double_hash(data)[:4])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rustc", type=Path, required=True)
    parser.add_argument("--work-directory", type=Path, required=True)
    parser.add_argument("--reference-directory", type=Path, required=True)
    parser.add_argument("--native-lib", type=Path, action="append", default=[])
    options = parser.parse_args()
    source = (Path(__file__).resolve().parent / "../src/bitcoin_encoding.rs").resolve()
    ref_path = options.reference_directory / "segwit_addr.py"
    reference_bytes = ref_path.read_bytes()
    if hashlib.sha256(reference_bytes).hexdigest() != REFERENCE_SHA256:
        raise RuntimeError("BIP reference changed; review and update its recorded hash before testing")
    spec = importlib.util.spec_from_file_location("bip_reference", ref_path)
    reference = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(reference)
    work = options.work_directory.resolve()
    work.mkdir(parents=True, exist_ok=True)
    harness = work / "differential_harness.rs"
    binary = work / "differential_harness.exe"
    harness.write_text(HARNESS.replace("__SOURCE_PATH__", json.dumps(str(source))), encoding="utf-8")
    compiler = [str(options.rustc), "--edition=2024", "-D", "warnings", "-C", "opt-level=2",
                "-C", "overflow-checks=yes", "-C", "target-feature=+crt-static", str(harness), "-o", str(binary)]
    for library in options.native_lib:
        compiler.extend(["-L", f"native={library}"])
    subprocess.run(compiler, check=True)
    random_data = random.Random(0x173350)
    cases = []

    def add(operation, fields, expected):
        cases.append(("\t".join([operation, *map(str, fields)]), expected))

    for length in [0, 1, 2, 3, 20, 32, 55, 56, 57, 63, 64, 65, 119, 120, 121, 127, 128, 129,
                   255, 256, 1024, 65537, 1_000_000]:
        data = random_data.randbytes(length)
        expected = hashlib.sha256(data).hexdigest()
        add("sha", [data.hex()], expected)
        add("sha_double", [data.hex()], double_hash(data).hex())
        for chunk in [1, 3, 7, 55, 56, 63, 64, 65, 127, 8191]:
            add("sha_fragment", [data.hex(), chunk], expected)
    for _ in range(300):
        data = random_data.randbytes(random_data.randrange(0, 4093))
        if random_data.randrange(2):
            data = b"\0" * random_data.randrange(0, 5) + data[:4087]
        b58 = base58_encode(data)
        check = base58check(data)
        add("hex", [data.hex().upper()], data.hex())
        add("base58", [data.hex()], b58)
        add("base58_decode", [b58], data.hex())
        add("check", [data.hex()], check)
        add("check_decode", [check], data.hex())
        if check:
            pos = random_data.randrange(len(check))
            changed = check[:pos] + random_data.choice(ALPHABET.replace(check[pos], "")) + check[pos + 1:]
            add("check_decode", [changed], "ERR")
    for _ in range(300):
        hrp = "".join(random_data.choice("abcdefghijk0123456789-._~") for _ in range(random_data.randrange(1, 30)))
        symbols = list(random_data.randbytes(random_data.randrange(0, 84 - len(hrp))))
        symbols = [value & 31 for value in symbols]
        variant = random_data.choice(list(reference.Encoding))
        text = reference.bech32_encode(hrp, symbols, variant)
        add("bech", [hrp, variant.value, bytes(symbols).hex()], text)
        add("bech_decode", [text], f"{hrp}\t{variant.value}\t{bytes(symbols).hex()}")
        add("bech_decode", [text.upper()], f"{hrp.upper()}\t{variant.value}\t{bytes(symbols).hex()}")
    for network, hrp in [("main", "bc"), ("test", "tb"), ("testnet4", "tb"), ("signet", "tb"), ("regtest", "bcrt")]:
        for version in range(17):
            for length in [2, 3, 20, 31, 32, 39, 40]:
                if version == 0 and length not in (20, 32):
                    continue
                program = random_data.randbytes(length)
                text = reference.encode(hrp, version, program)
                assert text is not None
                add("witness", [network, version, program.hex()], text)
                add("witness_decode", [network, text], f"{version}\t{program.hex()}")
                add("witness_decode", [network, text.upper()], f"{version}\t{program.hex()}")
                add("address_decode", [network, text], text)
                for _ in range(3):
                    pos = random_data.randrange(len(text))
                    changed = text[:pos] + random_data.choice(reference.CHARSET.replace(text[pos], "")) + text[pos + 1:]
                    expected_version, expected_program = reference.decode(hrp, changed)
                    expected = "ERR" if expected_version is None else f"{expected_version}\t{bytes(expected_program).hex()}"
                    add("witness_decode", [network, changed], expected)
    # Structured invalid padding, wrong variants, length and unknown versions
    # are checked both by the primary reference and by the actual Rust decoder.
    for _ in range(500):
        hrp = random_data.choice(["bc", "tb", "bcrt", "tc"])
        version = random_data.randrange(0, 32)
        symbols = [random_data.randrange(0, 32) for _ in range(random_data.randrange(0, 66))]
        spec = random_data.choice(list(reference.Encoding))
        text = reference.bech32_encode(hrp, [version, *symbols], spec)
        for network, expected_hrp in [("main", "bc"), ("test", "tb"), ("regtest", "bcrt")]:
            decoded_version, decoded_program = reference.decode(expected_hrp, text)
            expected = "ERR" if decoded_version is None else f"{decoded_version}\t{bytes(decoded_program).hex()}"
            add("witness_decode", [network, text], expected)
    input_text = "\n".join(line for line, _ in cases) + "\n"
    output = subprocess.run([str(binary)], input=input_text, text=True, capture_output=True, check=True)
    lines = output.stdout.splitlines()
    if len(lines) != len(cases):
        raise AssertionError(f"result count {len(lines)} differs from {len(cases)}")
    for index, ((line, expected), actual) in enumerate(zip(cases, lines)):
        if actual != expected:
            raise AssertionError(f"case {index}: {line[:180]!r}: expected {expected!r}, got {actual!r}")
    counts = Counter(line.split("\t", 1)[0] for line, _ in cases)
    summary = {
        "state": "pass",
        "seed": "0x173350",
        "cases": len(cases),
        "operations": dict(sorted(counts.items())),
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "reference_sha256": REFERENCE_SHA256,
        "sha_reference": "Python standard-library hashlib.sha256",
        "base58_reference": "Python arbitrary-precision integer conversion",
        "bech32_reference": "Pieter Wuille BIP173/BIP350 Python reference",
    }
    (work / "differential-results.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(summary, sort_keys=True))


if __name__ == "__main__":
    main()
