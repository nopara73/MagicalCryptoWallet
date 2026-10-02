"""Inventory retained packaged input and historical grammar; no runtime files changed."""
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]

def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, encoding="utf-8", stderr=subprocess.PIPE)

def sections(source):
    # Exact existing Announcements.ReleaseHighlights extraction/normalization.
    def extract(header):
        lines = source.split("\n")
        begin = next((i for i, line in enumerate(lines) if line.lstrip().startswith(header)), None)
        if begin is None:
            return ""
        result = []
        for line in lines[begin+1:]:
            if line.lstrip().startswith("## "):
                break
            result.append(line)
        return "\n".join(result).strip()
    summary = extract("## Release Highlights")
    release = extract("## Release Summary")
    lines = release.split("\n")
    while lines and not lines[0]:
        lines.pop(0)
    details = "\n".join(line.strip() for line in lines[1:]) if lines else ""
    return ("## Summary\n"+summary if summary.strip() else "")+"\n"+("## Details\n"+details if details.strip() else "")

def inventory(output):
    sources = [("current", "MagicalCryptoWallet/Announcements/ReleaseHighlights.md", git("show", "HEAD:MagicalCryptoWallet/Announcements/ReleaseHighlights.md"))]
    for commit in git("log", "-30", "--format=%H", "--", "WalletWasabi/Announcements/ReleaseHighlights.md").splitlines():
        try:
            source = git("show", commit+":WalletWasabi/Announcements/ReleaseHighlights.md")
        except subprocess.CalledProcessError:
            continue
        sources.append((commit, "WalletWasabi/Announcements/ReleaseHighlights.md", source))
    output.mkdir(parents=True, exist_ok=True)
    records = []
    patterns = {"heading": r"(?m)^#{1,6} ", "list": r"(?m)^\s*(?:[-+*]|\d+\.) ", "link": r"\[[^\]]+\]\(", "emphasis": r"\*[^\n]+\*", "code": r"`", "image": r"!\[", "table": r"(?m)^\|", "html": r"<[A-Za-z]"}
    for identity, path, source in sources:
        text = sections(source)
        fixture = identity+".md"
        (output/fixture).write_text(text, encoding="utf-8", newline="\n")
        records.append({"source_commit": identity, "source_path": path, "source_sha256": hashlib.sha256(source.encode()).hexdigest(), "derived_markdown_sha256": hashlib.sha256(text.encode()).hexdigest(), "fixture": fixture, "bytes": len(text.encode()), "grammar_counts": {name: len(re.findall(pattern, text)) for name, pattern in patterns.items()}})
    result = {"scope": "actual ReleaseHighlights.MarkdownText and historical release-note grammar; historical features are test data only", "head": git("rev-parse", "HEAD").strip(), "records": records}
    (output/"inventory.json").write_text(json.dumps(result, indent=2)+"\n", encoding="utf-8")
    print(json.dumps({"inputs": len(records), "current_bytes": records[0]["bytes"], "grammar": {name: sum(r["grammar_counts"][name] for r in records) for name in patterns}}, indent=2))

if __name__ == "__main__":
    inventory(ROOT/".artifacts/native-ui-markdown-inputs")
