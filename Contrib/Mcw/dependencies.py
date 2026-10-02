#!/usr/bin/env python3
"""Reproducible migration inventory of NuGet graphs, source and bundled payloads."""
import argparse, difflib, hashlib, itertools, json, re, sys, xml.etree.ElementTree as ET
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
DEST=ROOT/"Contrib/McwMigration/dependencies.json"
APPIMAGE_RUNTIME_SOURCE="https://github.com/AppImage/type2-runtime/blob/8f39b89e2ac31e1640b3d3f7e9a5108e6ce805fa/src/runtime/Makefile"

def group(name):
    if any(word in name for word in ("Avalonia","Reactive","DynamicData","Skia","HarfBuzz","Markdown","FlashCap","QRackers","MicroCom")): return "native-ui"
    if any(word in name for word in ("Sqlite","SQLite")): return "storage"
    if any(word in name for word in ("Json","Newtonsoft")): return "formats"
    if any(word in name for word in ("NBitcoin","Secp","WabiSabi")): return "wallet-cryptography"
    if any(word in name for word in ("Nostr","Http","WebUtilities")): return "network-privacy"
    if any(word in name.lower() for word in ("test","xunit","coverlet","codeanalysis","moq","castle")): return "verification-build"
    return "application-services"

def role(path):
    if path.startswith("Contrib/McwMigration/"): return "verification"
    if any(part in path for part in ("Tests","VisualPreview","BridgeProbe","AssemblyAudit")): return "verification"
    if "Coordinator" in path or "Backend" in path: return "external-service"
    if "Publisher" in path: return "release-tool"
    if "Generators" in path: return "build"
    return "application"

def inventory():
    rust_sources={path.stem:path for path in (ROOT/"mcw/src").glob("*.rs")
                  if path.stem not in {"app","bridge","command","lib","main","platform"}}
    rust_sources.update({path.parent.name:path for path in (ROOT/"mcw/src").glob("*/mod.rs")
                         if path.parent.name!="platform"})
    rust_components=[]
    for name,path in sorted(rust_sources.items()):
        handoff_name={"privacy_service":"privacy","script_service":"bitcoin-script",
                      "wallet_hash_service":"wallet-hmac","bitcoin_block_service":"bitcoin-block",
                      "psbt_metadata_service":"psbt-metadata","markdown":"native-ui",
                      "serialization_service":"json-rpc","scan_service":"qr-scanning",
                      "safe_file_service":"storage","content_service":"compression"}.get(name,name.replace("_","-"))
        handoff=ROOT/"Contrib/McwMigration/Handoffs"/(handoff_name+".md")
        rust_components.append({"name":name,"path":path.relative_to(ROOT).as_posix(),
            "status":"production callers migrated" if name=="qr" else "implementation present; production caller integration pending",
            "handoff":"MagicalCryptoWallet.Documentation/McwArchitecture.md" if name=="qr"
                       else handoff.relative_to(ROOT).as_posix() if handoff.is_file() else None})
    packages={}
    for lock in sorted(ROOT.rglob("packages.lock.json"),key=lambda path:path.relative_to(ROOT).as_posix()):
        relative=lock.relative_to(ROOT).as_posix()
        if any(part in relative.split("/") for part in (".artifacts","bin","obj","target")): continue
        for framework,values in json.loads(lock.read_text(encoding="utf-8-sig"))["dependencies"].items():
            # Per-RID restore sections repeat framework packages and are mutable
            # build outputs. Native payloads are tracked separately by all-target
            # bundle hashes, framework lock edges and extracted package audits.
            if "/" in framework: continue
            for name,entry in values.items():
                if entry.get("type")=="Project": continue
                key=(name,entry["resolved"])
                item=packages.setdefault(key,{"name":name,"version":entry["resolved"],"group":group(name),"status":"retained",
                    "uses":[],"dependencies":{},"source":"nuget-lock"})
                use={"lock":relative,"framework":framework,"type":entry["type"],"role":role(relative)}
                if use not in item["uses"]: item["uses"].append(use)
                item["dependencies"].update(entry.get("dependencies",{}))
    references=[]
    for project in sorted(ROOT.rglob("*.csproj"),key=lambda path:path.relative_to(ROOT).as_posix()):
        relative=project.relative_to(ROOT).as_posix()
        if any(part in relative.split("/") for part in (".artifacts","bin","obj")):continue
        for node in ET.parse(project).findall(".//PackageReference"):
            if name:=node.get("Include"): references.append({"name":name,"project":relative,"role":role(relative)})
    central=ET.parse(ROOT/"Directory.Packages.props")
    properties={node.tag:node.text for node in central.findall("./PropertyGroup/*")}
    pins=[]
    for node in central.findall(".//PackageVersion"):
        version=node.get("Version","")
        for name,value in properties.items(): version=version.replace("$("+name+")",value or "")
        pins.append({"name":node.get("Include"),"version":version,"used_reference":any(ref["name"]==node.get("Include") for ref in references)})
    bundles=[]
    for directory in ("MagicalCryptoWallet/BundledApps/Binaries","MagicalCryptoWallet.IntegrationTests/BundledApps/Binaries"):
        for path in sorted((ROOT/directory).rglob("*"),key=lambda path:path.relative_to(ROOT).as_posix()):
            if path.is_file() and not path.name.startswith("."):
                data=path.read_bytes()
                scope="file bytes"
                if path.name in {"LICENSE","LICENSE.md","NOTICE.md"}:
                    # Git checks out text with platform-specific line endings.
                    # Only notices are canonicalized; executable/library hashes
                    # always describe their original, unmodified bytes.
                    data=data.decode("utf-8").replace("\r\n","\n").encode("utf-8")
                    scope="UTF-8 text with LF line endings"
                bundles.append({"path":path.relative_to(ROOT).as_posix(),"bytes":len(data),
                                "sha256":hashlib.sha256(data).hexdigest(),"hash_scope":scope,
                                "role":role(directory),"status":"retained"})
    return {
        "schema":1,
        "application":"mcw",
        "policy":"Rust standard library and native OS APIs only; no external Cargo dependencies, companion Rust libraries or executables",
        "nuget":sorted(packages.values(),key=lambda x:(x["name"].lower(),x["version"])),
        "package_references":references,"central_pins":pins,"bundled_files":bundles,
        "rust_components":rust_components,
        "copied_source":[
            {"name":"Gma.QrCodeNet","path":"MagicalCryptoWallet/Gma/QrCodeNet","status":"removed","replacement":"mcw/src/qr.rs","evidence":"160 independent version/ECC decodes plus Unicode, numeric, URI and cancellation; source and package audits"},
            {"name":"Nito AsyncEx/Collections/Disposables","path":"MagicalCryptoWallet/Nito","status":"retained","group":"application-services"},
            {"name":"WabiSabi managed/native fork","path":"ThirdParty/WabiSabi","status":"retained","group":"wallet-cryptography","provenance":"ThirdParty/WabiSabi/UPSTREAM.json"}
        ] + ([{"name":"JSONTestSuite reference fixtures","path":"mcw/tests/json_vectors/JSONTestSuite","status":"verification-only oracle","provenance":"mcw/tests/json_vectors/README.md"}]
              if (ROOT/"mcw/tests/json_vectors/JSONTestSuite").is_dir() else [])
          + ([{"name":"Brotli RFC static dictionary and transform data","path":"mcw/src/content_service/data","status":"standard format data; caller integration pending","provenance":"mcw/src/content_service/data/PROVENANCE.md"}]
              if (ROOT/"mcw/src/content_service/data/PROVENANCE.md").is_file() else [])
          + ([{"name":"QR scanning character mapping data","path":"mcw/src/scan_service/charset_data.rs","status":"generated format data; caller integration pending","provenance":"mcw/tests/qr_scanning_charset_tables.py"}]
              if (ROOT/"mcw/src/scan_service/charset_data.rs").is_file() else []),
        "native_and_embedded":[
            {"name":".NET runtime and ASP.NET shared libraries","status":"retained","owner":"managed application/external coordinator","evidence":"self-contained dotnet publish and .deps.json"},
            {"name":"Avalonia native platform backends, Skia and HarfBuzz","status":"retained","owner":"managed UI","evidence":"NuGet locks plus published runtimes/*/native payloads"},
            {"name":"SQLite native e_sqlite3","status":"retained","owner":"managed storage","evidence":"SQLitePCLRaw bundle/provider/native NuGet locks"},
            {"name":"libwabisabi","status":"retained","owner":"managed credential protocol","evidence":"ThirdParty/WabiSabi/c/CMakeLists.txt"},
            {"name":"libsecp256k1","version":"0.7.1","status":"retained, statically embedded in libwabisabi","owner":"credential cryptography","evidence":"CMake FetchContent URL and checksum"},
            {"name":"Tor","version":"0.4.9.9 (Windows inventory)","status":"retained","owner":"privacy networking","evidence":"bundled tor.exe --version; per-platform binary hashes above"},
            {"name":"Libevent","version":"2.1.12-stable (Windows Tor)","status":"retained inside Tor","owner":"Tor","evidence":"tor.exe --version; dylib/so payloads above"},
            {"name":"OpenSSL","version":"3.5.6 (Windows Tor)","status":"retained inside Tor","owner":"Tor TLS/crypto","evidence":"tor.exe --version; libssl/libcrypto payloads above"},
            {"name":"zlib","version":"1.3.2 (Windows Tor)","status":"retained inside Tor","owner":"Tor compression","evidence":"tor.exe --version"},
            {"name":"libstdc++","status":"retained in Linux Tor payloads","owner":"legacy bundled native programs","evidence":"bundled libstdc++.so.6 hashes above"},
            {"name":"AppImage ELF launcher runtime","status":"retained in Linux AppImage packages; independent of the mcw payload","owner":"installation and launch","evidence":"Contrib/Releases/package.py linux_packages; appimagetool prepends the separately downloaded runtime","upstream_source":APPIMAGE_RUNTIME_SOURCE,"embedded_build":"actual per-package runtime source/version and component versions unverified; a packaging-tool checksum is not a runtime checksum"},
            *[{"name":name,"status":"declared static dependency of the retained AppImage runtime; actual packaged version unverified","owner":"AppImage launcher runtime","upstream_source":APPIMAGE_RUNTIME_SOURCE}
              for name in ("Squashfuse (including squashfuse_ll)","libfuse3","Zstandard","zlib (AppImage runtime)","mimalloc")],
            {"name":"musl libc (AppImage runtime)","status":"retained static C runtime in the upstream AppImage launcher build; actual packaged version unverified","owner":"AppImage launcher runtime","evidence":"https://github.com/AppImage/type2-runtime/blob/8f39b89e2ac31e1640b3d3f7e9a5108e6ce805fa/README.md"},
            {"name":"Generated AppImage AppRun shell launcher","status":"retained packaging entrypoint delegating to mcw","owner":"installation and launch","evidence":"Contrib/Releases/package.py; uses system sh/readlink/dirname"},
            {"name":"Bitcoin Core and embedded native libraries","status":"verification-only; not client runtime","owner":"independent regtest oracle","evidence":"integration BundledApps hashes; upstream Bitcoin Core build manifest required before any migration claim"},
            {"name":"HWI and embedded Python/device dependencies","status":"absent from current client after software-wallet-only removal; future hardware scope requires a fresh full inventory","owner":"future hardware integration","evidence":"MagicalCryptoWallet.Documentation/SoftwareWallet.md and source/package removal audits"},
            {"name":"Rust 1.99.0 standard library","status":"retained platform baseline","owner":"mcw","evidence":"mcw/rust-toolchain.toml; application Cargo graph empty; Windows std rebuilt with panic=abort"},
            {"name":"OS libraries/API sets","status":"permitted platform baseline","owner":"mcw/platform","evidence":"Contrib/Mcw/audit.py runtime import evidence"}
        ]
    }

if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--check",action="store_true");args=parser.parse_args()
    text=json.dumps(inventory(),indent=2,ensure_ascii=False)+"\n"
    if args.check:
        saved=DEST.read_text(encoding="utf-8") if DEST.exists() else ""
        if saved!=text:
            sys.stderr.writelines(itertools.islice(difflib.unified_diff(saved.splitlines(True),text.splitlines(True),
                fromfile="recorded inventory",tofile="current inventory"),120))
            raise SystemExit("Dependency inventory is stale; run python Contrib/Mcw/dependencies.py")
    else:
        DEST.parent.mkdir(parents=True,exist_ok=True);DEST.write_text(text,encoding="utf-8")
    print("Dependency inventory verified" if args.check else str(DEST))
