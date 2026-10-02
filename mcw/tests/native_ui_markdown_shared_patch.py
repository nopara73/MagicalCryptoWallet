"""Emit the narrow QR-owned incorporation patch without editing shared files."""
import difflib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HANDOFF = ROOT / "Contrib/McwMigration/Handoffs/native-ui-shared.patch"

def changes():
    lib = "mcw/src/lib.rs"
    source = (ROOT/lib).read_text(encoding="utf-8-sig")
    assert "pub mod markdown;" not in source
    yield lib, source, source.replace("pub mod json;\n", "pub mod json;\npub mod markdown;\n")
    app = "mcw/src/app.rs"
    source = (ROOT/app).read_text(encoding="utf-8-sig")
    before = "        bridge::QR => bridge::encode_qr(frame).write(output),\n"
    assert source.count(before) == 1
    after = before + """        crate::markdown::OPERATION => {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            match crate::markdown::dispatch(&frame.payload, &cancel) {
                Ok(payload) => frame.reply(payload).write(output),
                Err(error) => frame
                    .error(
                        match error {
                            crate::markdown::Error::InvalidInput => 1,
                            crate::markdown::Error::Limit => 2,
                            crate::markdown::Error::Cancelled => 4,
                        },
                        error.diagnostic(),
                    )
                    .write(output),
            }
        }
"""
    yield app, source, source.replace(before, after)
    for path, line in [
        ("MagicalCryptoWallet.Fluent/MagicalCryptoWallet.Fluent.csproj", '\t\t<PackageReference Include="Markdown.Avalonia" />\n'),
        ("Directory.Packages.props", '    <PackageVersion Include="Markdown.Avalonia" Version="11.0.3" />\n'),
    ]:
        source = (ROOT/path).read_text(encoding="utf-8-sig")
        assert source.count(line) == 1, path
        yield path, source, source.replace(line, "")

if __name__ == "__main__":
    HANDOFF.parent.mkdir(parents=True, exist_ok=True)
    HANDOFF.write_text("".join("diff --git a/"+path+" b/"+path+"\n"+"".join(difflib.unified_diff(
        before.splitlines(keepends=True), after.splitlines(keepends=True), fromfile="a/"+path,tofile="b/"+path))
        for path,before,after in changes()), encoding="utf-8", newline="\n")
    print(HANDOFF)
