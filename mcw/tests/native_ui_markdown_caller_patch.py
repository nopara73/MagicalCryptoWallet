"""Capture the pending atomic caller cutover; never edit another checkout."""
import difflib
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT/"Contrib/McwMigration/Handoffs/native-ui-caller.patch"
PATHS = ["MagicalCryptoWallet.Fluent/Views/Dialogs/ReleaseHighlights/ReleaseHighlightsDialogView.axaml",
         "MagicalCryptoWallet.Fluent/Views/Dialogs/ReleaseHighlights/ReleaseHighlightsDialogView.cs",
         "MagicalCryptoWallet.Fluent/Styles/Markdown.axaml"]
patch = []
for path in PATHS:
    before = subprocess.check_output(["git", "show", "HEAD:"+path], cwd=ROOT, encoding="utf-8")
    after = "" if path.endswith("Styles/Markdown.axaml") else (ROOT/path).read_text(encoding="utf-8-sig")
    patch.append("diff --git a/"+path+" b/"+path+"\n")
    if not after: patch.append("deleted file mode 100644\n")
    patch.extend(difflib.unified_diff(before.splitlines(keepends=True),after.splitlines(keepends=True),
                 fromfile="a/"+path,tofile="b/"+path if after else "/dev/null"))
OUTPUT.write_text("".join(patch), encoding="utf-8", newline="\n")
print(OUTPUT)
