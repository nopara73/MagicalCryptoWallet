"""Recreate offline primary-source vectors; Python stdlib, development-only.

Downloaded documents stay under ignored evidence. Only vector facts and their
source hashes are checked in; no reference implementation becomes shipped code.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import unicodedata
import urllib.request
import zipfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--trezor-commit", help="Reuse the manifest's pinned revision")
    parser.add_argument("--offline", action="store_true", help="Read previously downloaded evidence files")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    evidence = root / ".artifacts/wallet-hashes-evidence/primary-sources"
    fixtures = root / "mcw/tests/wallet_hashes_fixtures"
    evidence.mkdir(parents=True, exist_ok=True)
    fixtures.mkdir(parents=True, exist_ok=True)
    sources = []

    def fetch(name, url):
        if args.offline:
            body = (evidence / name).read_bytes()
        else:
            request = urllib.request.Request(url, headers={"User-Agent": "mcw-vector-verification"})
            with urllib.request.urlopen(request, timeout=30) as response:
                body = response.read()
        (evidence / name).write_bytes(body)
        sources.append({"file": name, "url": url, "bytes": len(body),
                        "sha256": hashlib.sha256(body).hexdigest()})
        return body

    ripemd = fetch("ripemd160.html", "https://homes.esat.kuleuven.be/~bosselae/ripemd160.html")
    fetch("rmd160-spec.txt", "https://homes.esat.kuleuven.be/~bosselae/ripemd/rmd160.txt")
    rfc4231 = fetch("rfc4231.txt", "https://www.rfc-editor.org/rfc/rfc4231.txt").decode()
    fetch("rfc8018.txt", "https://www.rfc-editor.org/rfc/rfc8018.txt")
    rfc7914 = fetch("rfc7914.txt", "https://www.rfc-editor.org/rfc/rfc7914.txt").decode()
    fetch("fips180-4.pdf", "https://csrc.nist.gov/files/pubs/fips/180-4/final/docs/fips180-4.pdf")
    nist_zip = fetch("shabytetestvectors.zip", "https://csrc.nist.gov/CSRC/media/Projects/Cryptographic-Algorithm-Validation-Program/documents/shs/shabytetestvectors.zip")
    trezor_commit = args.trezor_commit
    if not trezor_commit:
        metadata = json.loads(fetch("trezor-commit.json", "https://api.github.com/repos/trezor/python-mnemonic/commits/master"))
        trezor_commit = metadata["sha"]
    if not re.fullmatch(r"[0-9a-f]{40}", trezor_commit):
        raise ValueError("Exact Trezor commit required")
    trezor_base = f"https://raw.githubusercontent.com/trezor/python-mnemonic/{trezor_commit}/"
    trezor_vectors = json.loads(fetch("trezor-vectors.json", trezor_base + "vectors.json"))
    license_text = fetch("trezor-LICENSE", trezor_base + "LICENSE")
    (fixtures / "TREZOR-LICENSE").write_bytes(license_text)
    fetch("bip0039.mediawiki", "https://raw.githubusercontent.com/bitcoin/bips/master/bip-0039.mediawiki")
    fetch("slip0021.md", "https://raw.githubusercontent.com/satoshilabs/slips/master/slip-0021.md")

    # TSV: operation, key/password hex, message/salt hex, iterations, expected hex.
    rows = []

    def add(operation, key, message, iterations, digest):
        rows.append("\t".join((operation, key.hex(), message.hex(), str(iterations), digest)))

    published_ripemd = [
        (b"", "9c1185a5c5e9fc54612808977ee8f548b2258d31"),
        (b"a", "0bdc9d2d256b3ee9daae347be6f4dc835a467ffe"),
        (b"abc", "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"),
        (b"message digest", "5d0689ef49d2fae572b881b123a85ffa21595f36"),
        (b"abcdefghijklmnopqrstuvwxyz", "f71c27109c692c1b56bbdceb5b9d2865b3708dbc"),
        (b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq", "12a053384a9c0c88e405a06c27dcf49ada62eb2b"),
        (b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789", "b0e20b6e3116640286ed3a87a5713079b21f5189"),
        (b"1234567890" * 8, "9b752e45573d4b39f4dbd3323cab82bf63326bfb"),
    ]
    for message, digest in published_ripemd:
        if digest.encode() not in ripemd:
            raise ValueError("Published RIPEMD vector missing")
        add("ripemd160", b"", message, 0, digest)
    counts = {"ripemd160": len(published_ripemd)}

    with zipfile.ZipFile(io.BytesIO(nist_zip)) as archive:
        for name in ("SHA512ShortMsg.rsp", "SHA512LongMsg.rsp"):
            path = next(n for n in archive.namelist() if n.endswith(name))
            body = archive.read(path)
            (evidence / name).write_bytes(body)
            count = 0
            for bits, message, digest in re.findall(r"Len = (\d+)\s+Msg = ([0-9a-fA-F]+)\s+MD = ([0-9a-fA-F]+)", body.decode()):
                bits = int(bits)
                if bits % 8:
                    raise ValueError("Byte-oriented API cannot accept bit vectors")
                message = bytes.fromhex(message)[:bits // 8]
                if len(message) * 8 != bits or len(digest) != 128:
                    raise ValueError("Incomplete NIST vector")
                add("sha512", b"", message, 0, digest.lower())
                count += 1
            counts[name] = count
            sources.append({"file": name, "zip_member": path,
                            "sha256": hashlib.sha256(body).hexdigest(), "bytes": len(body)})

    hmac_cases = [
        (bytes([0x0b]) * 20, b"Hi There"),
        (b"Jefe", b"what do ya want for nothing?"),
        (bytes([0xaa]) * 20, bytes([0xdd]) * 50),
        (bytes(range(1, 26)), bytes([0xcd]) * 50),
        (bytes([0x0c]) * 20, b"Test With Truncation"),
        (bytes([0xaa]) * 131, b"Test Using Larger Than Block-Size Key - Hash Key First"),
        (bytes([0xaa]) * 131, b"This is a test using a larger than block-size key and a larger than block-size data. The key needs to be hashed before being used by the HMAC algorithm."),
    ]
    # Read only the section bodies, avoiding duplicated table-of-contents headings.
    for number, (key, message) in enumerate(hmac_cases, 1):
        section = re.search(rf"^4\.{number + 1}\.  Test Case {number}[ \t]*\n(.*?)(?=^4\.\d+\.  Test Case|^5\.  Security)", rfc4231, re.S | re.M)
        if not section:
            raise ValueError(f"RFC4231 case {number} missing")
        for algorithm in (256, 512):
            digest = re.search(rf"HMAC-SHA-{algorithm} = ([0-9a-f]+(?:\n +[0-9a-f]+)*)", section[1])[1]
            digest = "".join(digest.split())
            if len(digest) != (32 if number == 5 else algorithm // 4):
                raise ValueError("RFC4231 digest extraction failed")
            add(f"hmac{algorithm}", key, message, 0, digest)
    counts["rfc4231"] = 14

    # RFC7914 section 11 publishes two PBKDF2-HMAC-SHA256 cases.
    section = rfc7914[rfc7914.index("11.  Test Vectors for PBKDF2 with HMAC-SHA-256", 5000):]
    section = section[:section.index("12.  Test Vectors for scrypt")]
    for password, salt, iterations, length, digest_lines in re.findall(
            r'PBKDF2-HMAC-SHA-256 \(P="(.*?)", S="(.*?)",\s+c=(\d+), dkLen=(\d+)\) =\s*((?:[ \t]*[0-9a-f ]+\n)+)', section):
        digest = "".join(digest_lines.split())
        if len(digest) != int(length) * 2:
            raise ValueError("RFC7914 digest extraction failed")
        add("pbkdf2256", password.encode(), salt.encode(), int(iterations), digest)
    counts["rfc7914"] = sum(row.startswith("pbkdf2256\t") for row in rows)
    if counts["rfc7914"] != 2:
        raise ValueError("Expected both RFC7914 PBKDF2 cases")

    # BIP39: provide already normalized bytes to the primitive; this is no mnemonic implementation.
    for language, vectors in trezor_vectors.items():
        count = 0
        for _entropy, mnemonic, seed, _bip32 in vectors:
            password = unicodedata.normalize("NFKD", mnemonic).encode()
            add("pbkdf2512", password, b"mnemonicTREZOR", 2048, seed.lower())
            count += 1
        counts["bip39_" + language] = count
    body = ("# operation\tkey_or_password_hex\tmessage_or_salt_hex\titerations\texpected_hex\n" + "\n".join(rows) + "\n").encode()
    (fixtures / "vectors.tsv").write_bytes(body)
    manifest = {"schema_version": 1, "sources": sources, "trezor_commit": trezor_commit,
                "counts": counts, "vectors": len(rows), "fixture_sha256": hashlib.sha256(body).hexdigest(),
                "scope": "Published vector facts only; independent primitives are not vendored"}
    (fixtures / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps({"vectors": len(rows), "counts": counts, "fixture_sha256": manifest["fixture_sha256"]}))


if __name__ == "__main__":
    main()
