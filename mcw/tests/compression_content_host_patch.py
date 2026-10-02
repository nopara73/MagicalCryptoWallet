"""Prepare exact small shared patches from the published host/factory sources.
Never edits those source files or the active QR/network checkout.
"""
import argparse
import difflib
import hashlib
import json
from pathlib import Path


def once(text, old, new):
    assert text.count(old) == 1, (old[:100], text.count(old))
    return text.replace(old, new, 1)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--repo", required=True)
    p.add_argument("--out", required=True)
    args = p.parse_args()
    repo = Path(args.repo).resolve()
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    files = {}
    source_hashes = {}

    lib_path = "mcw/src/lib.rs"
    lib = (repo/lib_path).read_text(encoding="utf-8")
    assert "pub mod content_service;" not in lib
    files[lib_path] = once(lib, "pub mod compression;", "pub mod compression;\npub mod content_service;")

    app_path = "mcw/src/app.rs"
    app = (repo/app_path).read_text(encoding="utf-8")
    modified = once(app, "    platform,", "    content_service::{adapter as content_adapter, Abort},\n    platform,")
    modified = once(modified, "    sync::mpsc,", "    sync::{mpsc, Arc},")
    modified = once(modified, "    let (send, receive) = mpsc::sync_channel(16);", """    let (send, receive) = mpsc::sync_channel(16);
    let content_controls = Arc::new(content_adapter::Controls::new());
    let reader_controls = Arc::clone(&content_controls);""")
    modified = once(modified, "            let result = Frame::read(&mut output);", """            let result = Frame::read(&mut output).and_then(|frame| {
                if let Some(frame) = &frame
                    && frame.operation == content_adapter::OPERATION {
                    if frame.kind == bridge::REQUEST {
                        reader_controls.register(frame.id)
                            .map_err(|_| bridge::invalid("invalid content request ID"))?;
                    } else if frame.kind == bridge::CANCEL && frame.payload.is_empty() {
                        reader_controls.cancel(frame.id)
                            .map_err(|_| bridge::invalid("content cancellation unavailable"))?;
                    }
                }
                Ok(frame)
            });""")
    # Exactly the two existing host-loop dispatch calls receive their connection's controls.
    anchor = "                    bootstrap,\n                )"
    assert modified.count(anchor) == 2, "Host dispatch call topology changed"
    modified = modified.replace(anchor, "                    bootstrap,\n                    &content_controls,\n                )")
    modified = once(modified, "    bootstrap: &[u8],\n) -> io::Result<()> {", "    bootstrap: &[u8],\n    content_controls: &content_adapter::Controls,\n) -> io::Result<()> {")
    modified = once(modified, "        bridge::QR => bridge::encode_qr(frame).write(output),", """        bridge::QR => bridge::encode_qr(frame).write(output),
        content_adapter::OPERATION => {
            let payload = match content_adapter::execute_registered(
                frame.id, &frame.payload, content_controls, &mut || {
                    if *closing || platform::shutdown_requested() {
                        Err(Abort::Cancelled)
                    } else { Ok(()) }
                },
            ) {
                Ok(payload) => payload,
                Err(_) => return frame.error(0x0901, "content response allocation failed").write(output),
            };
            frame.reply(payload).write(output)
        },""")
    files[app_path] = modified

    factory_path = "MagicalCryptoWallet/WebClients/MagicalCryptoWallet/MagicalCryptoWalletHttpClientFactory.cs"
    factory = (repo/factory_path).read_text(encoding="utf-8")
    modified = once(factory, "using MagicalCryptoWallet.Logging;", "using MagicalCryptoWallet.Logging;\nusing MagicalCryptoWallet.Mcw.Content;")
    modified = once(modified, "ConcurrentDictionary<string, HttpClientHandler> _httpClientHandlers", "ConcurrentDictionary<string, HttpMessageHandler> _httpClientHandlers")
    modified = once(modified, "var httpClientHandler = _httpClientHandlers.GetOrAdd(name, CreateHttpClientHandler);", """var httpClientHandler = _httpClientHandlers.GetOrAdd(name, identity =>
        {
            var transport = CreateHttpClientHandler(identity);
            return identity == McwContentDecodingHandler.ClientName
                ? new McwContentDecodingHandler(transport)
                : transport;
        });""")
    modified = once(modified, "handler.AutomaticDecompression = DecompressionMethods.All;", """handler.AutomaticDecompression = name == McwContentDecodingHandler.ClientName
            ? DecompressionMethods.None : DecompressionMethods.All;""")
    files[factory_path] = modified

    host_diff = []
    factory_diff = []
    for name, text in files.items():
        before = (repo/name).read_text(encoding="utf-8")
        source_hashes[name] = hashlib.sha256((repo/name).read_bytes()).hexdigest()
        patch = list(difflib.unified_diff(before.splitlines(keepends=True), text.splitlines(keepends=True),
                                          fromfile="a/"+name, tofile="b/"+name))
        (factory_diff if name == factory_path else host_diff).extend(patch)
        destination = out/"review"/name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(text, encoding="utf-8", newline="\n")
    for name, lines in (("qr-content.patch", host_diff), ("network-content.patch", factory_diff)):
        (out/name).write_text("".join(lines), encoding="utf-8", newline="\n")
    manifest = dict(source_hashes=source_hashes,
                    patches={name:hashlib.sha256((out/name).read_bytes()).hexdigest()
                             for name in ("qr-content.patch", "network-content.patch")},
                    source_files_modified=False, active_checkouts_modified=False,
                    operation="0x0900", named_client="MempoolSpace-bitcoin-fee-rate-provider")
    (out/"patch-manifest.json").write_text(json.dumps(manifest, indent=2)+"\n", encoding="utf-8")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
