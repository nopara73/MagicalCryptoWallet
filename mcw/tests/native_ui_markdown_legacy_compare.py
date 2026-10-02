"""Compare real Rust dispatch/managed decode with immutable prior-renderer data.

No prior parser package or code is retained. Expectations were captured from
Markdown.Avalonia.Full 11.0.3; the fixture identifies every input and assembly.
Historical feature names in test data do not reintroduce a product feature.
"""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
EXPECTED = json.loads((Path(__file__).with_name("native_ui_markdown_legacy.json")).read_text())

def blocks(document):
    return [(block["Kind"], block["Level"], block["Depth"],
             [(run["Text"], run["Style"], run["Link"]) for run in block["Runs"]])
            for block in document["Blocks"]]

def compare():
    output = ROOT/".artifacts/native-ui-markdown-verification"
    results = []
    for record in EXPECTED["records"]:
        assert hashlib.sha256(record["markdown"].encode()).hexdigest() == record["markdown_sha256"]
        source = (ROOT/".artifacts/native-ui-markdown-inputs"/record["fixture"]).read_text(encoding="utf-8")
        assert source == record["markdown"], "Reference input changed: " + record["fixture"]
        actual = json.loads((output/"render"/(Path(record["fixture"]).stem+".json")).read_text())
        matched = blocks(actual) == blocks(record["expected"])
        results.append({"fixture": record["fixture"], "matched": matched, "blocks": len(actual["Blocks"])})
    (output/"legacy-comparison.json").write_text(json.dumps(results, indent=2)+"\n", encoding="utf-8")
    assert all(r["matched"] for r in results), "Legacy release-highlights presentation mismatch."
    print(f"Exact legacy semantic comparison: {len(results)}/{len(results)} inputs matched.")

if __name__ == "__main__":
    compare()
