"""Prepare reviewed content hooks against the host owner's bounded Inbox.

Writes only a fresh artifact directory in the content task. Active host/factory
sources are read-only and source-hashed; old sync_channel/Controls proposals are
historical evidence and are deliberately refused for new incorporation.
"""
import argparse
import difflib
import hashlib
import json
from pathlib import Path
import subprocess


def once(text, old, new):
    assert text.count(old) == 1, (old[:100], text.count(old))
    return text.replace(old, new, 1)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True, help="Owned content checkout and factory source")
    parser.add_argument("--host-repo", help="Read-only current host-owner checkout; defaults to repo")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    repo = Path(args.repo).resolve()
    host_repo = Path(args.host_repo).resolve() if args.host_repo else repo
    out = Path(args.out).resolve()
    assert out.is_relative_to(repo / ".artifacts"), "Review must stay in the owned artifacts directory"
    assert not out.exists(), "Use a fresh review; preserve earlier source-bound evidence"
    lib_path = "mcw/src/lib.rs"
    app_path = "mcw/src/app.rs"
    inbox_path = "mcw/src/app/inbox.rs"
    factory_path = "MagicalCryptoWallet/WebClients/MagicalCryptoWallet/MagicalCryptoWalletHttpClientFactory.cs"
    dependencies = ("mcw/src/bridge.rs", "mcw/src/qr.rs", "mcw/src/qr/tables.rs")
    assert (host_repo / inbox_path).is_file(), (
        "Current bounded Inbox required; legacy blocking-reader proposals are retired")
    roots = {name: host_repo for name in (lib_path, app_path, inbox_path, *dependencies)}
    roots[factory_path] = repo
    originals = {name: (root / name).read_bytes() for name, root in roots.items()}
    texts = {name: data.decode("utf-8").replace("\r\n", "\n") for name, data in originals.items()}
    app = texts[app_path]
    assert "let receive = Arc::new(Inbox::default());" in app, (
        "Current bounded Inbox required; legacy blocking-reader proposals are retired")
    assert "pub mod content_service;" not in texts[lib_path], "Use --integrated verification after incorporation"
    files = {lib_path: once(texts[lib_path], "pub mod compression;", "pub mod compression;\npub mod content_service;")}

    inbox = texts[inbox_path]
    assert "fn is_interrupted(" not in inbox, "Owner hook already exists; inspect its final contract"
    files[inbox_path] = once(inbox, "impl Inbox {\n", """impl Inbox {
    /// Synchronous services observe reader-side CANCEL and terminal ingress
    /// closure without waiting for dispatch to consume another event. Dispatch
    /// is the sole consumer while this query runs; request IDs pair with ops.
    pub fn is_interrupted(&self, id: u64, operation: u16) -> bool {
        let Ok(state) = self.state.lock() else {
            return true;
        };
        state.closed
            || state.cancellations.iter().any(|cancel| {
                cancel.id == id && cancel.operation == operation
            })
    }

""")
    modified = once(app, "    platform,", "    content_service::{adapter as content_adapter, Abort},\n    platform,")
    anchor = "                    bootstrap,\n                )"
    assert modified.count(anchor) == 2, "Host dispatch call topology changed"
    modified = modified.replace(anchor, "                    bootstrap,\n                    &receive,\n                )")
    modified = once(modified, "    bootstrap: &[u8],\n) -> io::Result<()> {",
                    "    bootstrap: &[u8],\n    inbox: &Inbox,\n) -> io::Result<()> {")
    files[app_path] = once(modified, "        bridge::QR => bridge::encode_qr(frame).write(output),", """        bridge::QR => bridge::encode_qr(frame).write(output),
        content_adapter::OPERATION => {
            let payload = match content_adapter::execute(&frame.payload, &mut || {
                if *closing || platform::shutdown_requested()
                    || inbox.is_interrupted(frame.id, frame.operation)
                {
                    Err(Abort::Cancelled)
                } else {
                    Ok(())
                }
            }) {
                Ok(payload) => payload,
                Err(_) => return frame.error(0x0901, "content response allocation failed").write(output),
            };
            frame.reply(payload).write(output)
        },""")

    factory = texts[factory_path]
    modified = once(factory, "using MagicalCryptoWallet.Logging;", "using MagicalCryptoWallet.Logging;\nusing MagicalCryptoWallet.Mcw.Content;")
    modified = once(modified, "ConcurrentDictionary<string, HttpClientHandler> _httpClientHandlers", "ConcurrentDictionary<string, HttpMessageHandler> _httpClientHandlers")
    modified = once(modified, "var httpClientHandler = _httpClientHandlers.GetOrAdd(name, CreateHttpClientHandler);", """var httpClientHandler = _httpClientHandlers.GetOrAdd(name, identity =>
        {
            var transport = CreateHttpClientHandler(identity);
            return identity == McwContentDecodingHandler.ClientName
                ? new McwContentDecodingHandler(transport)
                : transport;
        });""")
    files[factory_path] = once(modified, "handler.AutomaticDecompression = DecompressionMethods.All;", """handler.AutomaticDecompression = name == McwContentDecodingHandler.ClientName
            ? DecompressionMethods.None : DecompressionMethods.All;""")

    # A peer edit during preparation fails closed instead of mixing baselines.
    for name, root in roots.items():
        assert (root / name).read_bytes() == originals[name], "Source changed during review: " + name
    out.mkdir(parents=True)
    host_diff, factory_diff = [], []
    for name, data in originals.items():
        source = out / "source" / name
        source.parent.mkdir(parents=True, exist_ok=True)
        source.write_bytes(data)
        target = out / "review" / name
        target.parent.mkdir(parents=True, exist_ok=True)
        if name in files:
            target.write_text(files[name], encoding="utf-8", newline="\n")
            patch = list(difflib.unified_diff(texts[name].splitlines(keepends=True), files[name].splitlines(keepends=True),
                                            fromfile="a/" + name, tofile="b/" + name))
            (factory_diff if name == factory_path else host_diff).extend(patch)
        else:
            target.write_bytes(data)
    patches = {}
    for name, lines in (("qr-content.patch", host_diff), ("network-content.patch", factory_diff)):
        (out / name).write_text("".join(lines), encoding="utf-8", newline="\n")
        patches[name] = digest((out / name).read_bytes())
    for name, root in roots.items():
        assert (root / name).read_bytes() == originals[name], "Source changed during review: " + name
    manifest = dict(
        source_hashes={name: digest(data) for name, data in originals.items()},
        review_hashes={name: digest((out / "review" / name).read_bytes()) for name in originals},
        source_roots={name: str(root) for name, root in roots.items()},
        host_source_commit=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=host_repo).decode().strip(),
        factory_source_commit=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo).decode().strip(),
        patches=patches, source_files_modified=False, active_checkouts_modified=False,
        contract="bounded Inbox interruption query keyed by (id, operation); closed/poisoned aborts",
        retired_contract="sync_channel reader hook and duplicate 32-ID Controls",
        production_integrated=False, actual_application_host=False,
        operation="0x0900", named_client="MempoolSpace-bitcoin-fee-rate-provider",
    )
    (out / "patch-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
