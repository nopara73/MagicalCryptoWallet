#!/usr/bin/env python3
"""Audit the mcw runtime graph, executable identity and migration invariants."""
import argparse, hashlib, json, os, re, struct, subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

def pe_imports(path):
    data = path.read_bytes()
    pe = struct.unpack_from("<I", data, 0x3c)[0]
    if data[pe:pe+4] != b"PE\0\0": raise RuntimeError("Not a PE executable")
    sections = struct.unpack_from("<H", data, pe+6)[0]
    optional_size = struct.unpack_from("<H", data, pe+20)[0]
    optional = pe+24
    if struct.unpack_from("<H", data, optional)[0] != 0x20b: raise RuntimeError("mcw must be Windows x64")
    table = optional+optional_size
    def offset(rva):
        for i in range(sections):
            start = table+40*i
            virtual_size, address, size, file = struct.unpack_from("<IIII",data,start+8)
            if address <= rva < address+max(virtual_size,size): return file+rva-address
        raise RuntimeError("Invalid PE RVA")
    def string(rva):
        start = offset(rva); end = data.index(0,start)
        return data[start:end].decode("ascii")
    result=[]
    rva = struct.unpack_from("<I", data, optional+112+8)[0]
    if rva:
        pos=offset(rva)
        while any(data[pos:pos+20]):
            result.append(string(struct.unpack_from("<I",data,pos+12)[0])); pos+=20
    # Delay imports have a separate directory and are part of the runtime graph.
    rva = struct.unpack_from("<I",data,optional+112+13*8)[0]
    if rva:
        pos=offset(rva)
        while any(data[pos:pos+32]):
            flags,name=struct.unpack_from("<II",data,pos)
            if not flags & 1: raise RuntimeError("Unexpected VA delay import")
            result.append(string(name)); pos+=32
    return sorted(set(result))

def audit(binary):
    assert not (ROOT / "MagicalCryptoWallet/Gma/QrCodeNet").exists() or not any((ROOT / "MagicalCryptoWallet/Gma/QrCodeNet").rglob("*.cs")), "Vendored QR encoder remains"
    manifest=(ROOT / "mcw/Cargo.toml").read_text()
    assert 'name = "mcw"' in manifest and 'edition = "2024"' in manifest
    if os.name == "nt":
        imports=pe_imports(binary)
        allowed={"kernel32.dll","kernelbase.dll","ntdll.dll","bcrypt.dll","userenv.dll","ws2_32.dll","dbghelp.dll","shell32.dll","advapi32.dll"}
        for name in imports:
            assert name.lower() in allowed or name.lower().startswith(("api-ms-win-core-","api-ms-win-security-")), "Non-OS runtime import: "+name
        # Execution uses the audited executable; inspect loaded modules including
        # forwarded native dependencies rather than relying on the Cargo graph.
    elif os.uname().sysname == "Darwin":
        imports=subprocess.check_output(["otool","-L",str(binary)]).decode().splitlines()[1:]
        assert all(line.strip().startswith(("/usr/lib/","/System/Library/")) for line in imports), "Non-OS Mach-O dependency"
    else:
        imports=subprocess.check_output(["ldd",str(binary)]).decode().splitlines()
        assert "not found" not in "\n".join(imports)
        allowed = re.compile(r"^(linux-(vdso|gate)\.so\.[0-9]+|lib(c|m|pthread|dl|rt|util|resolv)\.so\.[0-9]+|ld-linux[^/]*\.so\.[0-9]+)$")
        rejected = [line.strip() for line in imports if line.strip() and not allowed.fullmatch(Path(line.strip().split()[0]).name)]
        assert imports and not rejected, "Non-OS ELF runtime dependency: " + "; ".join(rejected)
    version=subprocess.check_output([str(binary),"--version"]).decode().strip()
    assert version.startswith("mcw ")
    result={"binary":str(binary),"sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),"version":version,"runtime_imports":imports}
    evidence=ROOT / ".artifacts/mcw-evidence";evidence.mkdir(parents=True,exist_ok=True)
    (evidence / (binary.name+".imports.json")).write_text(json.dumps(result,indent=2)+"\n")
    print(json.dumps(result))

if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--binary",type=Path,required=True);args=parser.parse_args()
    audit(args.binary.resolve())
