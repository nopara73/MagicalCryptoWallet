"""Dev-only independent hashlib reference; never imported by the application."""
from pathlib import Path
import hashlib
import json
import sys

root = Path(__file__).resolve().parent
output = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "privacy_fixtures"
output.mkdir(parents=True, exist_ok=True)
vectors = []
for size in [0, 1, 2, 7, 8, 15, 16, 31, 32, 63, 64, 65, 127, 128, 135, 136, 137, 255, 256, 271, 272, 273, 1024, 8193]:
    data = bytes((i * 197 + size * 11) & 255 for i in range(size))
    vectors.append([data.hex(), hashlib.sha3_256(data).hexdigest(), hashlib.shake_256(data).hexdigest(300)])
output.joinpath("hashes.tsv").write_text("\n".join("\t".join(v) for v in vectors)+"\n", newline="\n")
output.joinpath("reference.json").write_text(json.dumps({
    "provider": "Python hashlib standard-library independent reference (its native backend is dev-only)",
    "python": sys.version,
    "shipping_dependency": False,
    "vectors": len(vectors),
    "sources": ["https://csrc.nist.gov/pubs/fips/202/final", "https://spec.torproject.org/rend-spec/encoding-onion-addresses.html"],
    "sha256": hashlib.sha256(output.joinpath("hashes.tsv").read_bytes()).hexdigest(),
}, indent=2)+"\n", newline="\n")
print(f"Generated {len(vectors)} independent SHA3/SHAKE vectors.")
