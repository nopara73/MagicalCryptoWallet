"""Prepare an ignored verification snapshot with the proposed host registration.

Shared/QR-owned source remains unchanged. The snapshot builds the same one mcw
crate, not a production sidecar or a second shipping library/executable.
"""
from pathlib import Path
import argparse
import shutil
from native_ui_markdown_shared_patch import ROOT, changes

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--output", type=Path, required=True)
OUTPUT = parser.parse_args().output.resolve()
if OUTPUT.exists():
    raise RuntimeError("Preserve the existing compiler snapshot; choose a fresh output path")
OUTPUT.mkdir(parents=True, exist_ok=False)
for name in ("Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml"):
    shutil.copy2(ROOT/"mcw"/name, OUTPUT/name)
for source in (ROOT/"mcw/src").rglob("*"):
    if source.is_file() and "native_ui" not in source.relative_to(ROOT/"mcw/src").parts:
        destination = OUTPUT/source.relative_to(ROOT/"mcw")
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
for path, before, after in changes():
    if path.startswith("mcw/"):
        (OUTPUT/Path(path).relative_to("mcw")).write_text(after, encoding="utf-8", newline="\n")
print(OUTPUT)
