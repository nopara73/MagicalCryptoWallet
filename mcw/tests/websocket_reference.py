"""Independent stdlib Python oracle; test tooling only, never shipped in mcw.

No production Rust encoder/parser is used to construct any expected result.
SHA-1/Base64 use independently implemented Python platform primitives. Frame
expectations use struct, codecs' incremental UTF-8 decoder and an explicit reader.
All inputs are deterministic synthetic bytes; no sockets/services/wallet files.
"""
import argparse
import base64
import codecs
import hashlib
import json
import random
import struct
import subprocess
from collections import Counter
from pathlib import Path

GUID = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
ACCEPT = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
OPS = {0, 1, 2, 8, 9, 10}
WIRE_CODES = set(range(1000, 1004)) | set(range(1007, 1015)) | set(range(3000, 5000))


class Reject(Exception):
    pass


def validate(op, final, data, role):
    if op == 1:
        try:
            codecs.getincrementaldecoder("utf-8")().decode(data, final=final)
        except UnicodeDecodeError:
            raise Reject("InvalidUtf8") from None
    if op == 8:
        if len(data) == 1:
            raise Reject("InvalidClose")
        if data:
            code, = struct.unpack("!H", data[:2])
            if code not in WIRE_CODES or (code == 1010 and role == "S"):
                raise Reject("InvalidClose")
            try:
                data[2:].decode("utf-8")
            except UnicodeDecodeError:
                raise Reject("InvalidUtf8") from None


def inspect(role, limit, wire):
    if not wire:
        return "MORE"
    flags = wire[0]
    if flags & 0x70:
        raise Reject("ReservedBits")
    op = flags & 0x0f
    if op not in OPS:
        raise Reject("InvalidOpcode")
    final = bool(flags & 0x80)
    if op >= 8 and not final:
        raise Reject("InvalidControl")
    if len(wire) < 2:
        return "MORE"
    masked = bool(wire[1] & 0x80)
    if masked != (role == "C"):
        raise Reject("WrongMask")
    size = wire[1] & 0x7f
    if op >= 8 and size > 125:
        raise Reject("InvalidControl")
    offset = 2
    if size == 126:
        if len(wire) < 4:
            return "MORE"
        size, = struct.unpack_from("!H", wire, offset)
        offset += 2
        if size < 126:
            raise Reject("NonCanonicalLength")
    elif size == 127:
        if len(wire) < 10:
            return "MORE"
        size, = struct.unpack_from("!Q", wire, offset)
        offset += 8
        if size >= 2**63:
            raise Reject("LengthOverflow")
        if size < 65536:
            raise Reject("NonCanonicalLength")
    if size > limit:
        raise Reject("FrameTooLarge")
    if masked:
        if len(wire) < offset + 4:
            return "MORE"
        key = wire[offset:offset + 4]
        offset += 4
    else:
        key = bytes(4)
    if len(wire) < offset + size:
        return "MORE"
    data = bytes(b ^ key[i % 4] for i, b in enumerate(wire[offset:offset + size]))
    validate(op, final, data, role)
    return f"OK|{op}|{int(final)}|{offset + size}|{data.hex()}"


def format_frame(role, op, final, data, mask, limit):
    if op not in OPS:
        raise Reject("InvalidOpcode")
    if (mask is not None) != (role == "C"):
        raise Reject("WrongMask")
    if op >= 8 and (not final or len(data) > 125):
        raise Reject("InvalidControl")
    if len(data) > limit:
        raise Reject("FrameTooLarge")
    validate(op, final, data, role)
    length = len(data)
    if length < 126:
        length_field = bytes([length])
    elif length < 65536:
        length_field = bytes([126]) + struct.pack("!H", length)
    else:
        length_field = bytes([127]) + struct.pack("!Q", length)
    if mask is not None:
        length_field = bytes([length_field[0] | 128]) + length_field[1:] + mask
        data = bytes(value ^ mask[i % 4] for i, value in enumerate(data))
    return bytes([op | (128 if final else 0)]) + length_field + data


def token(value):
    return bool(value) and all(c.isascii() and (c.isalnum() or c in "!#$%&'*+-.^_`|~") for c in value)


def handshake(status, offered, fields):
    if len(offered) > 32 or any(len(p.encode()) > 128 or not token(p) for p in offered) or len(offered) != len(set(offered)):
        raise Reject("InvalidSubprotocol")
    if status != 101:
        raise Reject("UnexpectedStatus")
    if len(fields) > 128:
        raise Reject("HandshakeTooLarge")
    singles = {}
    upgrade = False
    count = 0
    for name, value in fields:
        count += len(name.encode()) + len(value.encode())
        if count > 16384:
            raise Reject("HandshakeTooLarge")
        if len(name.encode()) > 256 or not token(name) or any(ord(c) == 127 or (ord(c) < 32 and c != "\t") for c in value):
            raise Reject("InvalidHeader")
        name = name.lower()
        value = value.strip(" \t")
        if name in {"upgrade", "sec-websocket-accept", "sec-websocket-protocol"}:
            if name in singles:
                raise Reject("DuplicateHeader")
            singles[name] = value
        elif name == "connection":
            for part in value.split(","):
                part = part.strip(" \t")
                if not token(part):
                    raise Reject("InvalidHeader")
                upgrade |= part.lower() == "upgrade"
        elif name == "sec-websocket-extensions":
            raise Reject("UnsupportedExtension")
    if singles.get("upgrade", "").lower() != "websocket":
        raise Reject("MissingUpgrade")
    if not upgrade:
        raise Reject("MissingConnectionUpgrade")
    if singles.get("sec-websocket-accept") != ACCEPT:
        raise Reject("InvalidAccept")
    selected = singles.get("sec-websocket-protocol")
    if selected is not None and (not token(selected) or selected not in offered):
        raise Reject("InvalidSubprotocol")
    return "OK|" + (selected.encode().hex() if selected is not None else "-")


def main():
    cli = argparse.ArgumentParser()
    cli.add_argument("--binary", required=True)
    cli.add_argument("--output", type=Path, required=True)
    args = cli.parse_args()
    rng = random.Random(0x64553174)
    requests, expected, counts = [], [], Counter()

    def add(kind, request, compute):
        try:
            answer = compute()
        except Reject as error:
            answer = "ERR|" + str(error)
        requests.append(request)
        expected.append(answer)
        counts[kind] += 1

    def frame(role, wire, limit=1024):
        add("parse_frame", f"F\t{role}\t{limit}\t{wire.hex()}", lambda: inspect(role, limit, wire))

    # Exhaustive header flags/length-marker/mask combinations with fixed extension
    # bytes, independent expectations and exact consumed/payload comparison.
    for first in range(256):
        for second in range(256):
            wire = bytes([first, second, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4])
            for role in ("C", "S"):
                frame(role, wire)
    for i in range(4096):
        nonce = bytearray(16)
        nonce[i // 256] = i % 256
        key = base64.b64encode(nonce)
        accept = base64.b64encode(hashlib.sha1(key + GUID).digest())
        add("handshake_digest", "H\t" + nonce.hex(), lambda key=key, accept=accept: key.decode() + "|" + accept.decode())
    for _ in range(2048):
        nonce = rng.randbytes(16)
        key = base64.b64encode(nonce)
        accept = base64.b64encode(hashlib.sha1(key + GUID).digest())
        add("handshake_digest", "H\t" + nonce.hex(), lambda key=key, accept=accept: key.decode() + "|" + accept.decode())
    text_data = [b"", b"Hello", "a\0é€😀\U0010ffff".encode(), b"\xc2", b"\xe2\x82", b"\xc0\x80", b"\xed\xa0\x80", b"\xf4\x90\x80\x80", b"\xff"]
    for role in ("C", "S"):
        for op in (0, 1, 2, 3, 8, 9, 10, 15):
            for final in (False, True):
                for data in text_data + [struct.pack("!H", n) for n in (999, 1000, 1004, 1005, 1006, 1007, 1010, 1014, 1015, 2000, 3000, 4999, 5000)]:
                    for mask in (None, bytes(4), bytes.fromhex("37fa213d")):
                        def encoded(role=role, op=op, final=final, data=data, mask=mask):
                            return "OK|" + format_frame(role, op, final, data, mask, 1024).hex()
                        mask_text = "-" if mask is None else mask.hex()
                        add("encode_frame", f"E\t{role}\t{op}\t{int(final)}\t{mask_text}\t{data.hex()}\t1024", encoded)
                        # Decode input constructed independently; reserved opcodes
                        # and malformed control shapes enter raw corpus separately.
                        try:
                            wire = format_frame(role, op, final, data, mask, 1024)
                        except Reject:
                            continue
                        for end in range(min(len(wire), 16)):
                            frame(role, wire[:end])
                        frame(role, wire + b"synthetic suffix")
    for length in (0, 1, 124, 125, 126, 127, 255, 256, 65535, 65536, 65537):
        data = rng.randbytes(length)
        for role in ("C", "S"):
            mask = rng.randbytes(4) if role == "C" else None
            wire = format_frame(role, 2, True, data, mask, 65537)
            for end in sorted({0, 1, 2, min(len(wire), 4), min(len(wire), 10), len(wire) // 2, len(wire) - 1, len(wire)}):
                frame(role, wire[:end], 65537)
            frame(role, wire, max(0, length - 1))
            add("encode_frame", f"E\t{role}\t2\t1\t{'-' if mask is None else mask.hex()}\t{data.hex()}\t65537", lambda wire=wire: "OK|" + wire.hex())
    for _ in range(4096):
        wire = rng.randbytes(rng.randrange(0, 48))
        frame(rng.choice(["C", "S"]), wire, rng.choice([0, 4, 125, 256, 65536]))
    base_fields = [("Upgrade", "websocket"), ("Connection", "Upgrade"), ("Sec-WebSocket-Accept", ACCEPT)]
    def check_handshake(status, offered, fields):
        protocol_arg = ",".join(p.encode().hex() for p in offered) if offered else "-"
        field_arg = ",".join(n.encode().hex() + ":" + v.encode().hex() for n, v in fields) if fields else "-"
        add("response_upgrade", f"G\t{status}\t{protocol_arg}\t{field_arg}", lambda: handshake(status, offered, fields))
    for status in range(100, 600):
        check_handshake(status, [], base_fields)
    for name in ("Upgrade", "Connection", "Sec-WebSocket-Accept", "Sec-WebSocket-Protocol", "Sec-WebSocket-Extensions", "X-Other", "bad:name"):
        for value in ("", "websocket", "WebSocket", "upgrade", " upgrade\t", "keep-alive, Upgrade", "upgrade,,keep-alive", "chat", "Chat", "chat, Chat", ACCEPT, ACCEPT + "=", "permessage-deflate", "invalid\nX: injected", "é", "\0"):
            check_handshake(101, ["chat"], base_fields + [(name, value)])
    for offered in ([], ["chat"], ["chat", "Chat"], [""], ["chat", "chat"], ["bad protocol"], ["a" * 129], ["p"] * 33):
        check_handshake(101, offered, base_fields)
    check_handshake(101, [], base_fields + [("X", "v")] * 126)
    check_handshake(101, [], base_fields + [("X", "a" * 16384)])
    input_text = "\n".join(requests) + "\n"
    expected_text = "\n".join(expected) + "\n"
    result = subprocess.run([args.binary], input=input_text, text=True, capture_output=True, timeout=120, check=False)
    if result.returncode:
        raise SystemExit(f"Rust actual-source oracle driver failed: {result.stderr[:1000]}")
    actual = result.stdout.splitlines()
    if len(actual) != len(expected):
        raise SystemExit(f"Output count mismatch {len(actual)} != {len(expected)}")
    for index, (got, want) in enumerate(zip(actual, expected)):
        if got != want:
            args.output.mkdir(parents=True, exist_ok=True)
            (args.output / "oracle-failure.txt").write_text(f"case {index}\n{requests[index][:1000]}\nexpected {want[:1000]}\nactual {got[:1000]}\n", encoding="utf-8")
            raise SystemExit(f"Differential mismatch at case {index}; see oracle-failure.txt")
    evidence = {
        "status": "passed", "cases": len(expected), "case_counts": dict(counts),
        "seed": "0x64553174", "input_sha256": hashlib.sha256(input_text.encode()).hexdigest(),
        "expected_sha256": hashlib.sha256(expected_text.encode()).hexdigest(),
        "actual_sha256": hashlib.sha256(("\n".join(actual) + "\n").encode()).hexdigest(),
        "reference": "independent Python stdlib hashlib/base64/struct/codecs; synthetic only",
        "driver": str(Path(args.binary).resolve()), "driver_sha256": hashlib.sha256(Path(args.binary).read_bytes()).hexdigest(),
        "oracle_source_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    }
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "oracle.json").write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(f"Independent oracle: {len(expected)} comparisons passed; {dict(counts)}")


if __name__ == "__main__":
    main()
