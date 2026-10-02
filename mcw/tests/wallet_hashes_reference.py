"""Actual-source differential probe against Python stdlib hashlib/hmac.

Run through wallet_hashes_verify.ps1, which holds a shared compiler-slot handle.
The executable and Rust harness are ignored test tooling, never shipped by mcw.
"""
import argparse
from collections import Counter
import hashlib
import hmac
import json
from pathlib import Path
import random
import ssl
import subprocess
import sys


PROBE = r'''
use std::io::{self, BufRead, Write};
use wallet_hashes::*;
fn decode(text: &str) -> Result<Vec<u8>, &'static str> {
    if !text.len().is_multiple_of(2) { return Err("invalid synthetic hex length"); }
    let digit = |c| match c {b'0'..=b'9'=>Ok(c-b'0'), b'a'..=b'f'=>Ok(c-b'a'+10), _=>Err("invalid synthetic hex")};
    text.as_bytes().as_chunks::<2>().0.iter().map(|p| Ok(digit(p[0])?*16+digit(p[1])?)).collect()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input=io::stdin(); let mut out=io::BufWriter::new(io::stdout().lock());
    for record in input.lock().lines() {
        let record=record?; let f: Vec<_>=record.split('\t').collect();
        if f.len()!=6 { return Err("invalid synthetic record".into()); }
        let key=decode(f[1])?; let data=decode(f[2])?;
        let iterations=f[3].parse::<u32>()?; let length=f[4].parse::<usize>()?; let step=f[5].parse::<usize>()?;
        let digest: Vec<u8>=match f[0] {
            "ripemd160"=> {
                if step==0 { ripemd160(&data)?.to_vec() } else {
                    let mut state=Ripemd160::new(); for part in data.chunks(step) { state.update(part)?; }
                    state.update(b"")?; state.finalize().to_vec()
                }
            },
            "sha512"=> {
                if step==0 { sha512(&data)?.to_vec() } else {
                    let mut state=Sha512::new(); for part in data.chunks(step) { state.update(part)?; }
                    state.update(b"")?; state.finalize().to_vec()
                }
            },
            "hash160"=> {
                if step==0 { hash160(&data)?.to_vec() } else {
                    let mut state=Hash160::new(); for part in data.chunks(step) { state.update(part)?; }
                    state.update(b"")?; state.finalize().to_vec()
                }
            },
            "hmac256"=> {
                if step==0 { hmac_sha256(&key,&data)?.to_vec() } else {
                    let mut state=HmacSha256::new(&key)?; for part in data.chunks(step) { state.update(part)?; }
                    state.update(b"")?; state.finalize()?.to_vec()
                }
            },
            "hmac512"=> {
                if step==0 { hmac_sha512(&key,&data)?.to_vec() } else {
                    let mut state=HmacSha512::new(&key)?; for part in data.chunks(step) { state.update(part)?; }
                    state.update(b"")?; state.finalize()?.to_vec()
                }
            },
            "pbkdf2256"=>pbkdf2_hmac_sha256(&key,&data,iterations,length)?,
            "pbkdf2512"=>pbkdf2_hmac_sha512(&key,&data,iterations,length)?,
            _=>return Err("unknown synthetic operation".into()),
        };
        for byte in digest { write!(out,"{byte:02x}")?; } writeln!(out)?;
    }
    Ok(())
}
'''


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rustc", required=True)
    parser.add_argument("--work-directory", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    work = args.work_directory.resolve()
    work.mkdir(parents=True, exist_ok=True)
    encoding = root / "mcw/src/bitcoin_encoding.rs"
    source = root / "mcw/src/wallet_hashes.rs"
    hashes = {"wallet_hashes": hashlib.sha256(source.read_bytes()).hexdigest(),
              "bitcoin_encoding": hashlib.sha256(encoding.read_bytes()).hexdigest()}
    harness = work / "actual-source-probe.rs"
    harness.write_text('#![forbid(unsafe_code)]\n' +
                       f'#[path="{encoding.as_posix()}"] pub mod bitcoin_encoding;\n' +
                       f'#[path="{source.as_posix()}"] pub mod wallet_hashes;\n' + PROBE,
                       encoding="utf-8", newline="\n")
    # Probe lifetime contains synthetic buffers only, never user wallet files.
    random_source = random.Random(0x0A002026)
    cases = []

    def add(operation, key, data, iterations=0, length=0, step=0):
        if operation == "ripemd160":
            expected = hashlib.new("ripemd160", data).digest()
        elif operation == "sha512":
            expected = hashlib.sha512(data).digest()
        elif operation == "hash160":
            expected = hashlib.new("ripemd160", hashlib.sha256(data).digest()).digest()
        elif operation.startswith("hmac"):
            expected = hmac.digest(key, data, "sha" + operation[4:])
        else:
            expected = hashlib.pbkdf2_hmac("sha" + operation[6:], key, data, iterations, length)
        cases.append(("\t".join((operation, key.hex(), data.hex(), str(iterations), str(length), str(step))), expected.hex(), operation))

    sizes = list(range(260)) + [511, 512, 513, 1023, 1024, 1025, 4095, 4096, 4097, 65536, 1_048_576]
    for length in sizes:
        data = random_source.randbytes(length)
        steps = [0, 1, 7, 63, 127] if length < 2048 else [0, 137]
        for operation in ("ripemd160", "sha512", "hash160"):
            for step in steps:
                add(operation, b"", data, step=step)
    key_sizes = [0, 1, 16, 20, 31, 32, 33, 55, 56, 63, 64, 65, 111, 112, 127, 128, 129, 131, 255, 256, 257, 1024]
    message_sizes = [0, 1, 31, 32, 55, 56, 63, 64, 65, 111, 112, 127, 128, 129, 255, 256, 257]
    for key_size in key_sizes:
        key = random_source.randbytes(key_size)
        for length in message_sizes:
            data = random_source.randbytes(length)
            for operation in ("hmac256", "hmac512"):
                for step in (0, 1, 67):
                    add(operation, key, data, step=step)
    for _ in range(100):
        key = random_source.randbytes(random_source.randrange(513))
        data = random_source.randbytes(random_source.randrange(8193))
        for operation in ("hmac256", "hmac512"):
            add(operation, key, data, step=random_source.choice((0, 13, 128)))
    for operation in ("pbkdf2256", "pbkdf2512"):
        for key, salt in [(b"", b""), (b"password", b"salt"), (b"pass\0word", b"sa\0lt"),
                          (bytes(range(256)), bytes(range(255, -1, -1)))]:
            for iterations in (1, 2, 3, 7, 100):
                for length in (1, 31, 32, 33, 63, 64, 65, 127, 128, 129):
                    add(operation, key, salt, iterations, length)
        for key_size in (63, 64, 65, 127, 128, 129):
            for salt_size in (51, 52, 55, 56, 63, 64, 107, 108, 111, 112, 127, 128, 129):
                add(operation, random_source.randbytes(key_size), random_source.randbytes(salt_size), 3, 97)
        for _ in range(100):
            add(operation, random_source.randbytes(random_source.randrange(257)),
                random_source.randbytes(random_source.randrange(257)), random_source.randrange(1, 101),
                random_source.randrange(1, 193))
        add(operation, b"synthetic", b"salt", 1, 65_536)
        add(operation, random_source.randbytes(1_048_576), b"salt", 1, 65)
        add(operation, b"synthetic", random_source.randbytes(1_048_576), 1, 65)
        add(operation, b"synthetic", b"salt", 1_000_000, 1)
    payload = ("\n".join(record for record, _, _ in cases) + "\n").encode()
    (work / "synthetic-differential-input.tsv").write_bytes(payload)
    profiles = []
    for profile, optimization in (("debug", "0"), ("optimized", "3")):
        binary = work / f"wallet-hashes-probe-{profile}.exe"
        subprocess.run([args.rustc, "--edition=2024", "-D", "warnings", "-C", "codegen-units=1",
                        "-C", "overflow-checks=yes", "-C", "target-feature=+crt-static", "-C",
                        f"opt-level={optimization}", str(harness), "-o", str(binary)], check=True)
        result = subprocess.run([str(binary)], input=payload, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, timeout=180, check=True)
        actual = result.stdout.decode().splitlines()
        if len(actual) != len(cases):
            raise AssertionError("Differential response count mismatch")
        for index, (observed, (_, expected, operation)) in enumerate(zip(actual, cases)):
            if observed != expected:
                raise AssertionError(f"{profile} {operation} differential mismatch at synthetic case {index}")
        profiles.append({"profile": profile, "passed": len(cases),
                         "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest()})
    summary = {"python": sys.version, "reference_engine": ssl.OPENSSL_VERSION,
               "reference": "Python stdlib hashlib/hmac; independent development-only reference, not shipped",
               "source_sha256": hashes, "cases": len(cases), "counts": dict(Counter(case[2] for case in cases)),
               "profiles": profiles, "input_sha256": hashlib.sha256(payload).hexdigest(),
               "harness_sha256": hashlib.sha256(harness.read_bytes()).hexdigest()}
    (work / "differential-results.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
