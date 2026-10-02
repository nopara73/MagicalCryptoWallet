"""Read-only Tor caller/package/import audit. No relay or Tor process starts."""
from pathlib import Path
import hashlib
import json
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]

def cstring(data, offset):
    if offset < 0 or offset >= len(data):
        raise ValueError("invalid native string offset")
    end = data.find(b"\0", offset, min(offset+4096, len(data)))
    if end < 0:
        raise ValueError("unterminated native string")
    return data[offset:end].decode("ascii", "strict")

def pe_imports(data):
    pe = struct.unpack_from("<I", data, 0x3c)[0]
    if data[pe:pe+4] != b"PE\0\0":
        raise ValueError("invalid PE signature")
    count = struct.unpack_from("<H", data, pe+6)[0]
    optional_size = struct.unpack_from("<H", data, pe+20)[0]
    optional = pe+24
    magic = struct.unpack_from("<H", data, optional)[0]
    directories = optional+(112 if magic == 0x20b else 96)
    sections = []
    for index in range(count):
        at = optional+optional_size+index*40
        size, virtual, raw_size, raw = struct.unpack_from("<IIII", data, at+8)
        sections.append((virtual, max(size, raw_size), raw, raw_size))
    def offset(address):
        for virtual, length, raw, raw_size in sections:
            if virtual <= address < virtual+length and address-virtual < raw_size:
                return raw+address-virtual
        raise ValueError("PE RVA outside file-backed section")
    imports = []
    rva, size = struct.unpack_from("<II", data, directories+8)
    if rva:
        at = offset(rva)
        for i in range(min(size//20, 4096)):
            row = struct.unpack_from("<IIIII", data, at+i*20)
            if not any(row):
                break
            imports.append(cstring(data, offset(row[3])))
    delay, size = struct.unpack_from("<II", data, directories+13*8)
    if delay:
        at = offset(delay)
        for i in range(min(size//32, 4096)):
            row = struct.unpack_from("<IIIIIIII", data, at+i*32)
            if not any(row):
                break
            if row[0] & 1:
                imports.append(cstring(data, offset(row[1])))
            else:
                raise ValueError("unhandled VA-based PE delayed import")
    return {"format": "PE", "imports": sorted(set(imports))}

def elf_imports(data):
    if data[4] != 2:
        raise ValueError("only ELF64 supported")
    endian = "<" if data[5] == 1 else ">"
    phoff = struct.unpack_from(endian+"Q", data, 32)[0]
    entry, count = struct.unpack_from(endian+"HH", data, 54)
    loads, dynamic = [], None
    for i in range(count):
        kind, flags, offset, virtual, physical, size, memory, alignment = struct.unpack_from(endian+"IIQQQQQQ", data, phoff+i*entry)
        if kind == 1:
            loads.append((virtual, size, offset))
        if kind == 2:
            dynamic = (offset, size)
    if dynamic is None:
        return {"format": "ELF64", "imports": []}
    tags = {}
    needed = []
    for at in range(dynamic[0], dynamic[0]+dynamic[1], 16):
        tag, value = struct.unpack_from(endian+"QQ", data, at)
        if tag == 0:
            break
        if tag == 1:
            needed.append(value)
        tags[tag] = value
    address = tags.get(5)
    for virtual, size, offset in loads:
        if address is not None and virtual <= address < virtual+size:
            start = offset+address-virtual
            result = {"format": "ELF64", "imports": sorted(cstring(data, start+i) for i in needed)}
            result["runtime_paths"] = [cstring(data, start+tags[tag]) for tag in (15,29) if tag in tags]
            return result
    raise ValueError("ELF string table unmapped")

def macho_imports(data):
    if data[:4] in (b"\xca\xfe\xba\xbe", b"\xca\xfe\xba\xbf"):
        large = data[3] == 0xbf
        count = struct.unpack_from(">I", data, 4)[0]
        arches = []
        for i in range(count):
            at = 8+i*(32 if large else 20)
            if large:
                cpu, subtype, offset, size, align, reserved = struct.unpack_from(">IIQQII",data,at)
            else:
                cpu, subtype, offset, size, align = struct.unpack_from(">IIIII", data, at)
            arches.append({"cpu":cpu, **macho_imports(data[offset:offset+size])})
        return {"format":"Mach-O universal", "architectures":arches}
    endian = "<" if data[:4] == b"\xcf\xfa\xed\xfe" else ">"
    if data[:4] not in (b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf"):
        raise ValueError("unsupported Mach-O")
    count = struct.unpack_from(endian+"I", data, 16)[0]
    at, imports = 32, []
    for _ in range(count):
        command, size = struct.unpack_from(endian+"II", data, at)
        if size < 8 or at+size > len(data):
            raise ValueError("invalid Mach-O command")
        if command in (0xc,0x80000018,0x8000001f,0x20,0x80000023):
            name = struct.unpack_from(endian+"I",data,at+8)[0]
            imports.append(cstring(data,at+name))
        at += size
    return {"format":"Mach-O64", "imports":sorted(imports)}

def native_info(data):
    if data[:2] == b"MZ":
        return pe_imports(data)
    if data[:4] == b"\x7fELF":
        return elf_imports(data)
    if data[:4] in (b"\xcf\xfa\xed\xfe",b"\xfe\xed\xfa\xcf",b"\xca\xfe\xba\xbe",b"\xca\xfe\xba\xbf"):
        return macho_imports(data)
    return {"format":"data", "imports":[]}

paths = subprocess.check_output(["git","ls-tree","-r","--name-only","HEAD","MagicalCryptoWallet/BundledApps/Binaries"],cwd=ROOT,text=True).splitlines()
binary_paths = [p for p in paths if "/Tor/" in p]
entries = []
for path in binary_paths:
    data = ROOT.joinpath(path).read_bytes()
    entry = {"path":path,"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest(),"status":"retained; not retired"}
    try:
        entry.update(native_info(data))
    except (ValueError,struct.error,UnicodeError) as error:
        entry["audit_error"] = str(error)
    entries.append(entry)
callers = []
for directory in ["MagicalCryptoWallet","MagicalCryptoWallet.Client","MagicalCryptoWallet.Coordinator","MagicalCryptoWallet.Fluent"]:
    for path in ROOT.joinpath(directory).rglob("*.cs"):
        for number,line in enumerate(path.read_text(encoding="utf-8-sig").splitlines(),1):
            if any(term in line for term in ("TorManager", "TorProcessManager", "TorControlClient", "CreateEphemeralOnionService", "CreateOnionService", "SocksSettingsBehavior", "DnsSocksResolver", "TorBridges", "TorSettings", "TorStatusChecker")):
                callers.append({"path":path.relative_to(ROOT).as_posix(),"line":number,"code":line.strip()})
result = {
    "source_commit":subprocess.check_output(["git","rev-parse","HEAD"],cwd=ROOT,text=True).strip(),
    "native_entries":entries,"callers":callers,
    "live_network_tests":False,"production_integrated":False,"old_implementation_retired":False,"dependency_removed":False,
    "targets":{target:"runtime execution unverified" for target in ["Windows x64","Linux x64","Linux ARM64","macOS x64","macOS ARM64"]},
    "note":"Imports establish dynamic references only; static implementation provenance remains Tor build/license plus version evidence. No Tor process or public connection was started."
}
output = Path(sys.argv[1]) if len(sys.argv)>1 else ROOT/"mcw/tests/privacy_fixtures/inventory.json"
output.parent.mkdir(parents=True,exist_ok=True)
output.write_text(json.dumps(result,indent=2)+"\n",newline="\n")
print(f"Audited {len(entries)} tracked Tor package files and {len(callers)} caller lines; {sum('audit_error' in e for e in entries)} import parse errors.")
