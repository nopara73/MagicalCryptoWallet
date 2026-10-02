"""Produce the exact deferred host/caller patch from an immutable published base.

Shared host and managed surfaces are edited only when the integration owner
applies the bundle, or inside a dedicated temporary verification checkout.
"""
import argparse
import difflib
import subprocess
from pathlib import Path

BASE = "7ae424b5f5f3734ca1870962a2d913769c59b26d"
ADAPTER_PATH = "MagicalCryptoWallet/Mcw/BitcoinAddressValidation.cs"
ADAPTER = r'''using System;
using System.Buffers.Binary;
using System.Diagnostics.CodeAnalysis;
using System.IO;
using System.Text;
using NBitcoin;

namespace MagicalCryptoWallet.Mcw;

/// <summary>Typed adapter for Rust address validation. No managed text-decoder fallback.</summary>
public static class BitcoinAddressValidation
{
    public const ushort Operation = 0x020c;
    private static readonly UTF8Encoding Utf8 = new(false, true);

    public static bool TryGetScriptPubKey(string? address, Network network, [NotNullWhen(true)] out byte[]? scriptPubKey)
    {
        scriptPubKey = null;
        if (address is null) { return false; }
        byte networkId;
        switch (network.Name)
        {
            case "Main": networkId = 0; break;
            case "TestNet": networkId = 1; break;
            case "TestNet4": networkId = 2; break;
            case "signet": networkId = 3; break;
            case "RegTest": networkId = 4; break;
            default: return false;
        }
        int length;
        try { length = Utf8.GetByteCount(address); }
        catch (EncoderFallbackException) { return false; }
        if (length > 90) { return false; }
        var payload = new byte[length + 1];
        payload[0] = networkId;
        Utf8.GetBytes(address, payload.AsSpan(1));
        var service = McwApplicationServices.Current;
        var result = service.RequestAsync(Operation, payload, service.Stopped).GetAwaiter().GetResult();
        if (result.Length == 3 && result[0] == 0)
        {
            var reason = BinaryPrimitives.ReadUInt16LittleEndian(result.AsSpan(1));
            if (reason is >= 1 and <= 5) { return false; }
        }
        if (result.Length < 2 || result[0] != 1 || !IsStandardScript(result.AsSpan(1)))
        { throw new IOException("Invalid mcw address validation response."); }
        scriptPubKey = result[1..];
        return true;
    }

    private static bool IsStandardScript(ReadOnlySpan<byte> script)
    {
        if (script.Length == 25 && script[0] == 0x76 && script[1] == 0xa9 && script[2] == 20
            && script[23] == 0x88 && script[24] == 0xac) { return true; }
        if (script.Length == 23 && script[0] == 0xa9 && script[1] == 20 && script[22] == 0x87) { return true; }
        if (script.Length is < 4 or > 42 || script[1] != script.Length - 2) { return false; }
        return script[0] == 0 ? script[1] is 20 or 32 : script[0] is >= 0x51 and <= 0x60;
    }
}
'''


def blob(root, commit, path):
    return subprocess.check_output(["git", "show", f"{commit}:{path}"], cwd=root).decode("utf-8").replace("\r\n", "\n")


def once(text, old, new):
    if text.count(old) != 1:
        raise RuntimeError("Expected exact shared integration context is absent or ambiguous")
    return text.replace(old, new, 1)


def changes(root, base):
    paths = ["mcw/src/lib.rs", "mcw/src/app.rs", "MagicalCryptoWallet/Extensions/NBitcoinExtensions.cs"]
    original = {path: blob(root, base, path) for path in paths}
    updated = dict(original)
    updated[paths[0]] = once(updated[paths[0]], "pub mod bitcoin_block;\n",
        '#[path = "bitcoin_encoding/address_service.rs"]\npub mod bitcoin_address_service;\npub mod bitcoin_block;\n')
    updated[paths[1]] = once(updated[paths[1]], "        bridge::QR => bridge::encode_qr(frame).write(output),\n",
        "        bridge::QR => bridge::encode_qr(frame).write(output),\n"
        "        crate::bitcoin_address_service::VALIDATE_ADDRESS => {\n"
        "            crate::bitcoin_address_service::handle(frame).write(output)\n"
        "        }\n")
    old = '''\t/// <remarks>NBitcoin does not provide a try-parse method.</remarks>
\tpublic static bool TryParseBitcoinAddressForNetwork(string address, Network network, [NotNullWhen(true)] out BitcoinAddress? bitcoinAddress)
\t{
\t\ttry
\t\t{
\t\t\tbitcoinAddress = Network.Parse<BitcoinAddress>(address, network);
\t\t\treturn true;
\t\t}
\t\tcatch
\t\t{
\t\t\tbitcoinAddress = null;
\t\t\treturn false;
\t\t}
\t}
'''
    new = '''\t/// <remarks>Rust validates address text; managed Script/address objects retain their existing ownership.</remarks>
\tpublic static bool TryParseBitcoinAddressForNetwork(string address, Network network, [NotNullWhen(true)] out BitcoinAddress? bitcoinAddress)
\t{
\t\tbitcoinAddress = null;
\t\tif (!MagicalCryptoWallet.Mcw.BitcoinAddressValidation.TryGetScriptPubKey(address, network, out var script))
\t\t{
\t\t\treturn false;
\t\t}
\t\tbitcoinAddress = new Script(script).GetDestinationAddress(network);
\t\treturn bitcoinAddress is not null;
\t}
'''
    updated[paths[2]] = once(updated[paths[2]], old, new)
    original[ADAPTER_PATH] = ""
    updated[ADAPTER_PATH] = ADAPTER
    return original, updated


def main():
    args = argparse.ArgumentParser()
    args.add_argument("--base-commit", default=BASE)
    args.add_argument("--output", type=Path, default=Path(__file__).with_name("bitcoin_encoding_integration.patch"))
    options = args.parse_args()
    root = Path(__file__).resolve().parents[2]
    original, updated = changes(root, options.base_commit)
    patch = ""
    for path in updated:
        patch += f"diff --git a/{path} b/{path}\n"
        if not original[path]:
            patch += "new file mode 100644\n"
        patch += "".join(difflib.unified_diff(original[path].splitlines(True), updated[path].splitlines(True),
            fromfile=f"a/{path}" if original[path] else "/dev/null", tofile=f"b/{path}"))
    options.output.write_text(patch, encoding="utf-8", newline="\n")
    print(f"Patch base={options.base_commit}, paths={len(updated)}")


if __name__ == "__main__":
    main()
