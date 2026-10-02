"""Prepare the owned review patch and synthetic host cases; never edit host files."""
import argparse
import difflib
import hashlib
import json
from pathlib import Path
import struct

import compact_filters_reference as reference


def file_diff(name, before, after):
    before = before.replace("\r\n", "\n")
    after = after.replace("\r\n", "\n")
    return "".join(difflib.unified_diff(before.splitlines(keepends=True),
        after.splitlines(keepends=True), fromfile="a/" + name if before else "/dev/null",
        tofile="b/" + name))


def prepare(root, reference_directory, output_directory):
    tests = root / "mcw/tests"
    bridge_name = "mcw/src/bridge.rs"
    app_name = "mcw/src/app.rs"
    caller_name = "MagicalCryptoWallet/Wallets/WalletFilterProcessor.cs"
    adapter_name = "MagicalCryptoWallet/Mcw/CompactFilters/McwCompactFilterMatcher.cs"
    bridge = (root / bridge_name).read_text().replace("\r\n", "\n")
    app = (root / app_name).read_text().replace("\r\n", "\n")
    caller = (root / caller_name).read_text().replace("\r\n", "\n")
    assert "COMPACT_FILTER_MATCH_ANY" not in bridge
    assert "pub const QR: u16 = 1;" in bridge
    assert bridge.count("\n#[cfg(test)]\n") == 1
    handler = (tests / "compact_filters_bridge_handler.inc").read_text()
    changed_bridge = bridge.replace("pub const QR: u16 = 1;", "pub const QR: u16 = 1;\npub const COMPACT_FILTER_MATCH_ANY: u16 = 0x0702;")
    changed_bridge = changed_bridge.replace("\n#[cfg(test)]\n", "\n" + handler + "\n#[cfg(test)]\n")
    dispatch = "        bridge::QR => bridge::encode_qr(frame).write(output),"
    assert app.count(dispatch) == 1
    changed_app = app.replace(dispatch, dispatch + "\n        bridge::COMPACT_FILTER_MATCH_ANY => bridge::match_basic_compact_filter(frame).write(output),")
    old_call = "matchFound = filter.Filter.MatchAny(toTestKeys, filter.FilterKey);"
    assert caller.count(old_call) == 1
    changed_caller = caller.replace(old_call,
        "if (filter.Filter.P != 19 || filter.Filter.M != 784_931)\n"
        "\t\t\t{\n"
        "\t\t\t\tthrow new System.InvalidOperationException(\"Wallet block filters require BIP158 basic parameters.\");\n"
        "\t\t\t}\n"
        "\t\t\tmatchFound = await MagicalCryptoWallet.Mcw.CompactFilters.McwCompactFilterMatcher.MatchAnyAsync(\n"
        "\t\t\t\tfilter.FilterData, filter.Header.BlockHash.ToBytes(), toTestKeys, cancellationToken).ConfigureAwait(false);")
    adapter = (tests / "compact_filters_managed_adapter.inc").read_text()
    patch = file_diff(bridge_name, bridge, changed_bridge) + file_diff(app_name, app, changed_app)
    patch += file_diff(adapter_name, "", adapter) + file_diff(caller_name, caller, changed_caller)
    (tests / "compact_filters_host_wiring.patch").write_text(patch, encoding="utf-8", newline="\n")

    official_bytes = (reference_directory / "testnet-19.json").read_bytes()
    assert hashlib.sha256(official_bytes).hexdigest() == "d9049756f744e561b882a8eff507582fb7cd74ed9cf5542bdac58257449ee2a2"
    rows = json.loads(official_bytes)
    cases = []
    for row in rows:
        if len(row) < 7:
            continue
        height, display_hash, block_text, prevouts, _, filter_hex = row[:6]
        block_hash = bytes.fromhex(display_hash)[::-1]
        encoded = bytes.fromhex(filter_hex)
        outputs, _ = reference.block_outputs(bytes.fromhex(block_text))
        included = [s for s in outputs if s and s[0] != 106] + [bytes.fromhex(s) for s in prevouts if s]
        built, values = reference.construct(block_hash[:16], included, 19, 784931)
        assert built == encoded
        for label, queries in [("included", included), ("empty", []),
            ("absent", [b"MCW SYNTHETIC ABSENT QUERY"]), ("mixed", [b"MCW SYNTHETIC ABSENT QUERY"] + included[:2])]:
            expected = any((reference.siphash(block_hash[:16], q) * (len(set(included)) * 784931)) >> 64 in values for q in queries) if values else False
            cases.append(dict(name=f"official-{height}-{label}", hash=block_hash.hex(), filter=encoded.hex(),
                queries=[q.hex() for q in queries], expected=expected, error=False))
        for queries in [included[:1], []]:
            cases.append(dict(name=f"official-{height}-malformed-suffix-{len(queries)}", hash=block_hash.hex(),
                filter=(encoded + b"\0").hex(), queries=[q.hex() for q in queries], expected=False, error=True))
    # Valid framing with deliberately invalid inner length/count/trailing data.
    raw = []
    valid = bytes.fromhex(cases[0]["hash"]) + struct.pack("<I", len(bytes.fromhex(cases[0]["filter"]))) + bytes.fromhex(cases[0]["filter"]) + struct.pack("<I", 0)
    raw += [b"", b"\0" * 31, b"\0" * 32 + struct.pack("<I", 0xFFFFFFFF),
        b"\0" * 32 + struct.pack("<I", 1) + b"\0" + struct.pack("<I", 65537), valid + b"\0",
        valid[:-4] + struct.pack("<I", 1), valid[:-4] + struct.pack("<I", 1) + struct.pack("<I", 0xFFFFFFFF)]
    output_directory.mkdir(parents=True, exist_ok=True)
    (output_directory / "host-cases.json").write_text(json.dumps(dict(cases=cases, raw_errors=[x.hex() for x in raw]), indent=2), encoding="utf-8", newline="\n")
    print(f"Prepared exact four-file owner patch, {len(cases)} typed host cases and {len(raw)} malformed payloads.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--reference-directory", type=Path, required=True)
    parser.add_argument("--output-directory", type=Path, required=True)
    args = parser.parse_args()
    prepare(args.root, args.reference_directory, args.output_directory)
