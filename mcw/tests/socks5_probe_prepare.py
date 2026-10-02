"""Produce the bounded host/caller patch; optionally apply in a private worktree."""
from pathlib import Path
import argparse
import difflib

root = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--apply-private", action="store_true")
args = parser.parse_args()
if args.apply_private and not (root / ".git").is_file():
    raise SystemExit("Apply is allowed only in an isolated Git worktree.")

changes = []
def change(path, before, after):
    if before == after:
        raise SystemExit("Patch produced no change: " + path)
    changes.append((path, before, after))

path = "mcw/src/app.rs"
before = (root / path).read_text(encoding="utf-8")
marker = '        bridge::QR => bridge::encode_qr(frame).write(output),\n'
assert before.count(marker) == 1
arm = '''        crate::socks5::probe_service::PROBE => {
            match crate::socks5::probe_service::execute(
                frame.operation,
                &frame.payload,
                &crate::socks5::transport::Cancellation::new(),
            ) {
                Ok(response) => frame.reply(response.to_vec()).write(output),
                Err(cause) => frame.error(cause.code(), cause.message()).write(output),
            }
        }
'''
change(path, before, before.replace(marker, marker + arm))

path = "MagicalCryptoWallet/Mcw/Network/McwSocksProbe.cs"
assert not (root / path).exists()
change(path, "", (root / "mcw/tests/socks5_probe_adapter.cs").read_text(encoding="utf-8"))

path = "MagicalCryptoWallet/Tor/TorProcessManager.cs"
before = (root / path).read_text(encoding="utf-8")
after = before.replace("using System.Net.Sockets;\n", "using MagicalCryptoWallet.Mcw.Network;\n")
start = after.index("\tprivate static readonly byte[] NoAuthHandshakeMsg")
end = after.index("\tpublic TorProcessManager", start)
after = after[:start] + after[end:]
start = after.index("\tpublic virtual async Task<bool> IsTorRunningAsync")
end = after.index("\n\t/// <summary>", start)
method = '''\tpublic virtual async Task<bool> IsTorRunningAsync(CancellationToken cancellationToken)
\t{
\t\ttry
\t\t{
\t\t\tvar result = await McwSocksProbe.CheckAsync(_settings.SocksEndpoint, cancellationToken).ConfigureAwait(false);
\t\t\tif (!result.IsReady)
\t\t\t{
\t\t\t\tLogger.LogInfo($"Tor SOCKS5 readiness probe failed: {result.Failure}.");
\t\t\t}
\t\t\t_eventBus.Publish(new TorConnectionStateChanged(result.IsReady));
\t\t\treturn result.IsReady;
\t\t}
\t\tcatch (IOException)
\t\t{
\t\t\tLogger.LogInfo("Tor SOCKS5 readiness service is unavailable.");
\t\t\t_eventBus.Publish(new TorConnectionStateChanged(false));
\t\t\treturn false;
\t\t}
\t}
'''
after = after[:start] + method + after[end:]
change(path, before, after)

parts = []
for path, before, after in changes:
    parts.append(f"diff --git a/{path} b/{path}\n")
    if not before:
        parts.append("new file mode 100644\n")
    parts.extend(difflib.unified_diff(before.splitlines(keepends=True), after.splitlines(keepends=True), fromfile="a/" + path if before else "/dev/null", tofile="b/" + path, n=3))
patch = root / "mcw/tests/socks5_probe_caller.patch"
patch.write_text("".join(parts), encoding="utf-8", newline="\n")
if args.apply_private:
    for path, before, after in changes:
        target = root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(after, encoding="utf-8", newline="\n")
print("Bounded patch: " + str(patch))
