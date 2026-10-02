"""Independent development facts for bounded handlers; never shipped or imported by Rust."""
import hashlib
import hmac
import json
import pathlib
import random
import ssl

ROOT = pathlib.Path(__file__).resolve().parent
OUT = ROOT / "wallet_hmac_fixtures"
OUT.mkdir(exist_ok=True)
rng = random.Random(0x4D4357484D4143)
lengths = list(range(140)) + [255, 256, 257, 1023, 1024, 1025, 4096]
rows = []
for length in lengths:
    key = rng.randbytes(32)
    message = rng.randbytes(length)
    rows.append(("0a10", key + message, hmac.digest(key, message, "sha256")))
    rows.append(("0a11", message, hmac.digest(b"Symmetric key seed", message, "sha512")))
    rows.append(("0a12", key + message, hmac.digest(key, b"\0" + message, "sha512")))
fixture = "".join(f"{op}\t{payload.hex()}\t{result.hex()}\n" for op, payload, result in rows)
path = OUT / "independent.tsv"
path.write_text(fixture, encoding="ascii", newline="\n")
limit = 1_048_560
maxima = {
    "ownership": hmac.digest(bytes(32), bytes(limit - 32), "sha256").hex(),
    "seed": hmac.digest(b"Symmetric key seed", bytes(limit), "sha512").hex(),
    "child": hmac.digest(bytes(32), b"\0" + bytes(limit - 32), "sha512").hex(),
}
manifest = {
    "facts": "Python stdlib hashlib/hmac independent development reference",
    "openssl": ssl.OPENSSL_VERSION,
    "cases": len(rows),
    "per_operation": len(lengths),
    "fixture_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    "maximum_request_bytes": limit,
    "maximum_zero_payload_results": maxima,
    "production_reference_dependency": False,
}
(OUT / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="ascii", newline="\n")
(OUT / ".gitattributes").write_text("*.tsv text eol=lf\n*.json text eol=lf\n", encoding="ascii", newline="\n")
print(json.dumps(manifest))
